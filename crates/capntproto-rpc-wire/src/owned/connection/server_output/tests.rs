use super::*;
use capnp::{word, Word};
use capntproto_rpc_engine::{connection as storage, ids::QuestionId};
use core::num::NonZeroUsize;
const ZERO: Word = word(0, 0, 0, 0, 0, 0, 0, 0);
type Peer<'p, 'id, 's> = Driver<'p, 'id, 's, (), u32, u8, u32, 4, 4>;

#[test]
fn borrowed_capability_tables_roll_back_lookup_failures_without_allocating() {
    use core::cell::Cell;
    struct Table<'a, 'id> {
        entries: &'a [Option<&'a clients::Client<'id>>],
        reads: [Cell<u8>; 3],
        fail: Option<(usize, u8)>,
    }
    impl<'id> CapabilityTable<'id> for Table<'_, 'id> {
        fn len(&self) -> usize {
            self.entries.len()
        }
        fn capability(&self, index: usize) -> capnp::Result<Option<&clients::Client<'id>>> {
            let reads = &self.reads[index];
            reads.set(reads.get() + 1);
            if self.fail == Some((index, reads.get())) {
                return Err(capnp::Error::from_kind(capnp::ErrorKind::Overloaded));
            }
            Ok(self.entries[index])
        }
    }
    let measured = allocation_counter::measure(|| {
        with_driver(|mut driver, messages| {
            let receipt = driver.admit_import(ImportId::from_wire(42), ()).unwrap();
            let remote = driver.bind_import(&receipt).unwrap();
            driver.release_import(receipt);
            let local = driver
                .runtime
                .endpoint(binding::Route::local(73), clients::Location::Local)
                .unwrap();
            let entries = [Some(&local), None, Some(&remote)];
            let words = [
                word(1, 0, 0, 0, 30, 0, 0, 0),
                word(3, 0, 0, 0, 0, 0, 0, 0),
                ZERO,
                word(3, 0, 0, 0, 2, 0, 0, 0),
            ];
            let segments = [Word::words_to_bytes(&words)];
            let reader = capnp::message::Reader::new(
                capnp::message::SegmentArray::new(&segments),
                Default::default(),
            );
            let params = reader.get_root().unwrap();
            let method = calls::DynamicCall {
                no_promise_pipelining: true,
                ..Default::default()
            };
            let mut queue_slots = [None];
            let mut queue = messages::Queue::<()>::new(&mut queue_slots).unwrap();
            // Fail after a grant has been staged, then during final table
            // inspection. Neither failure may consume an output or ID.
            for fail in [Some((1, 1)), Some((1, 2))] {
                let table = Table {
                    entries: &entries,
                    reads: core::array::from_fn(|_| Cell::new(0)),
                    fail,
                };
                assert!(matches!(
                    driver.try_send_immediate_with_capability_table(
                        &remote,
                        method,
                        params,
                        &table,
                        messages,
                        &mut queue,
                        512,
                        Default::default(),
                        &mut [0; 8],
                        (),
                        |_, _, _| u8::MAX,
                    ),
                    Err(Error::Input(crate::owned::Error::Wire(
                        capnp::ErrorKind::Overloaded
                    )))
                ));
                assert!(queue.is_empty());
                assert_eq!(driver.ledgers.questions.len(), 0);
                for id in 0..8 {
                    assert!(driver
                        .ledgers
                        .exports
                        .lookup(ExportId::from_wire(id))
                        .is_none());
                }
            }
            let mapping = capnp::wire_capability::IndexMap::identity(3);
            for fail_index in 0..3 {
                let prefix = driver
                    .runtime
                    .endpoint(
                        binding::Route::local(101 + fail_index as u32),
                        clients::Location::Local,
                    )
                    .unwrap();
                let entries = [Some(&prefix), None, Some(&remote)];
                let table = Table {
                    entries: &entries,
                    reads: core::array::from_fn(|_| Cell::new(0)),
                    fail: Some((fail_index, 1)),
                };
                assert!(matches!(
                    driver.enqueue_dynamic_to_table(
                        &remote,
                        method,
                        &table,
                        messages,
                        512,
                        Default::default(),
                        calls::CallDestination::Caller,
                        |field| field.copy_with_capabilities(params, mapping),
                    ),
                    Err(Error::Input(crate::owned::Error::Wire(
                        capnp::ErrorKind::Overloaded
                    )))
                ));
                assert_eq!(driver.runtime.queued(), 0);
                assert!(driver.ledgers.questions.is_empty());
                driver.runtime.release(prefix);
                assert!(
                    matches!(driver.runtime.take_retired(), Some(binding::Route::Capability(binding::Endpoint::Local(owner))) if owner == 101 + fail_index as u32),
                    "failed admission cannot retain its prefix"
                );
            }
            {
                use capntproto_rpc_engine::completion;
                use core::{
                    future::Future,
                    pin::Pin,
                    task::{Context, Poll, Waker},
                };
                let mut reply = completion::Slot::new();
                let (sender, mut receiver) = reply.pair().unwrap();
                let mut slots = [InvocationSlot::EMPTY];
                let mut invocations = Invocations::new(&mut slots).unwrap();
                let table = Table {
                    entries: &entries,
                    reads: core::array::from_fn(|_| Cell::new(0)),
                    fail: Some((2, 1)),
                };
                let rejected = invocations
                    .enqueue_dynamic_to_table(
                        &mut driver,
                        sender,
                        &remote,
                        method,
                        &table,
                        messages,
                        512,
                        Default::default(),
                        calls::CallDestination::Caller,
                        |field| field.copy_with_capabilities(params, mapping),
                    )
                    .expect_err("lookup failure");
                assert!(matches!(
                    rejected.error,
                    Error::Input(crate::owned::Error::Wire(capnp::ErrorKind::Overloaded))
                ));
                assert!(invocations.is_empty());
                assert_eq!(driver.runtime.queued(), 0);
                assert!(rejected.sender.send(Err(capnp::ErrorKind::Failed)).is_ok());
                assert!(matches!(
                    Pin::new(&mut receiver).poll(&mut Context::from_waker(Waker::noop())),
                    Poll::Ready(Ok(Err(capnp::ErrorKind::Failed)))
                ));
            }
            let table = Table {
                entries: &entries,
                reads: core::array::from_fn(|_| Cell::new(0)),
                fail: None,
            };
            let key = driver
                .try_send_immediate_with_capability_table(
                    &remote,
                    method,
                    params,
                    &table,
                    messages,
                    &mut queue,
                    512,
                    Default::default(),
                    &mut [0; 8],
                    (),
                    |_, _, _| u8::MAX,
                )
                .unwrap()
                .unwrap();
            // Three entries occupy a four-entry inline buffer: no lookup may
            // inspect its unused suffix or re-read an already-staged grant.
            assert_eq!(table.reads.map(|reads| reads.get()), [1, 2, 1]);
            drain(&mut queue, messages, |root| {
                let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                    panic!("Call")
                };
                let table = call.unwrap().get_params().unwrap().get_cap_table().unwrap();
                assert_eq!(table.len(), 3);
                assert!(matches!(
                    table.get(0).which().unwrap(),
                    rpc_capnp::cap_descriptor::SenderHosted(_)
                ));
                assert!(matches!(
                    table.get(1).which().unwrap(),
                    rpc_capnp::cap_descriptor::None(())
                ));
                assert!(matches!(
                    table.get(2).which().unwrap(),
                    rpc_capnp::cap_descriptor::ReceiverHosted(42)
                ));
            });
            driver.cancel(key).unwrap();
            driver.runtime.release(local);
            driver.runtime.release(remote);
            close(driver, messages, &mut queue);
        });
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn imported_clients_use_ledger_capacity_without_a_second_binding_budget() {
    let measured = allocation_counter::measure(|| {
        let mut bytes = [[ZERO; 128]; 6];
        let mut buffers = bytes
            .each_mut()
            .map(|b| messages::Buffer::new(Word::words_to_bytes_mut(b)));
        messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
            storage::with_domain(|domain| {
                // Two dispatch slots, thirty-two import slots. A binding does
                // not spend a dispatch-sized slot or allocate another table.
                let mut slots = FixedStorage::<(), u32, u8, u32, 4, 4, 2, 64, 0, 32>::new();
                let mut driver = slots.bind(domain, 1).unwrap();
                let mut clients = [const { None }; 32];
                for (index, slot) in clients.iter_mut().enumerate() {
                    let receipt = driver
                        .admit_import(ImportId::from_wire(u32::MAX - index as u32), ())
                        .unwrap();
                    let client = driver.bind_import(&receipt).unwrap();
                    let alias = driver.bind_import(&receipt).unwrap();
                    assert_eq!(client.key(), alias.key());
                    assert_eq!(
                        driver.ledgers.imports.client_binding(receipt.key()),
                        Some(client.key())
                    );
                    driver.runtime.release(alias);
                    driver.release_import(receipt);
                    *slot = Some(client);
                }
                for client in clients.into_iter().flatten() {
                    driver.runtime.release(client);
                }
                let mut queue_slots = [None];
                close(
                    driver,
                    messages,
                    &mut messages::Queue::<()>::new(&mut queue_slots).unwrap(),
                );
            });
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn imported_client_links_retry_exhaustion_and_rebind_stale_graph_slots() {
    let measured = allocation_counter::measure(|| {
        with_driver(|mut driver, messages| {
            let receipt = driver
                .admit_import(ImportId::from_wire(u32::MAX), ())
                .unwrap();
            let held = core::array::from_fn::<_, 24, _>(|_| {
                driver
                    .runtime
                    .endpoint(binding::Route::local(0), clients::Location::Local)
                    .unwrap()
            });
            assert!(matches!(
                driver.bind_import(&receipt),
                Err(binding::Error::Graph(clients::Error::Full))
            ));
            assert!(driver
                .ledgers
                .imports
                .client_binding(receipt.key())
                .is_none());
            assert!(!driver.ledgers.imports.has_pending_release());
            for client in held {
                driver.runtime.release(client);
            }
            while let Some(owner) = driver.runtime.take_retired() {
                assert_eq!(
                    owner.release(&mut driver.ledgers.imports, &mut driver.ledgers.questions),
                    Some(0)
                );
            }
            let first = driver.bind_import(&receipt).unwrap();
            let original = first.key();
            driver.runtime.release(first);
            let owner = driver.runtime.take_retired().unwrap();
            assert!(owner
                .release(&mut driver.ledgers.imports, &mut driver.ledgers.questions)
                .is_none());
            let replacement = driver
                .runtime
                .endpoint(binding::Route::local(0), clients::Location::Local)
                .unwrap();
            assert_ne!(replacement.key(), original);
            let rebound = driver.bind_import(&receipt).unwrap();
            assert_ne!(rebound.key(), original);
            assert_ne!(rebound.key(), replacement.key());
            assert!(
                matches!(driver.runtime.inspect(&rebound).unwrap(), clients::Destination::Endpoint { owner: binding::Route::Capability(binding::Endpoint::Imported(remote)), .. } if remote.id() == ImportId::from_wire(u32::MAX))
            );
            let alias = driver.bind_import(&receipt).unwrap();
            assert_eq!(alias.key(), rebound.key());
            driver.runtime.release(alias);
            driver.runtime.release(rebound);
            driver.runtime.release(replacement);
            driver.release_import(receipt);
            let mut queue_slots = [None];
            close(
                driver,
                messages,
                &mut messages::Queue::<()>::new(&mut queue_slots).unwrap(),
            );
        })
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn immediate_tables_roll_back_and_refund_without_allocating_or_pinning_frames() {
    let measured = allocation_counter::measure(|| {
        for native in [false, true] {
            for canceled in [false, true] {
                for release in [false, true] {
                    with_driver(|mut driver, messages| {
                        if native {
                            driver.use_native_writer();
                        }
                        let imported = driver.admit_import(ImportId::from_wire(42), ()).unwrap();
                        let remote = driver.bind_import(&imported).unwrap();
                        driver.release_import(imported);
                        let local = driver
                            .runtime
                            .endpoint(binding::Route::local(73), clients::Location::Local)
                            .unwrap_or_else(|_| panic!("local"));
                        let words = [
                            word(1, 0, 0, 0, 38, 0, 0, 0),
                            word(3, 0, 0, 0, 0, 0, 0, 0),
                            ZERO,
                            word(3, 0, 0, 0, 2, 0, 0, 0),
                            word(3, 0, 0, 0, 3, 0, 0, 0),
                        ];
                        let segments = [Word::words_to_bytes(&words)];
                        let reader = capnp::message::Reader::new(
                            capnp::message::SegmentArray::new(&segments),
                            Default::default(),
                        );
                        let params = reader.get_root::<capnp::any_pointer::Reader>().unwrap();
                        let caps = [Some(&local), None, Some(&remote), Some(&local)];
                        let method = calls::DynamicCall {
                            interface: 7,
                            method: 9,
                            no_promise_pipelining: true,
                            only_promise_pipeline: false,
                        };
                        let older = driver
                            .enqueue_dynamic(
                                &remote,
                                method,
                                &[],
                                messages,
                                128,
                                Default::default(),
                                |_| Ok(()),
                            )
                            .unwrap();
                        let mut slots = [None];
                        let mut queue = messages::Queue::<()>::new(&mut slots).unwrap();
                        assert!(driver
                            .try_send_immediate_with_capabilities(
                                &remote,
                                method,
                                params,
                                &caps,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 8],
                                (),
                                |_, _, _| u8::MAX
                            )
                            .unwrap()
                            .is_none());
                        assert!(driver.cancel_invocation(older, messages));
                        let mut full = messages::Queue::<()>::new(&mut []).unwrap();
                        assert!(driver
                            .try_send_immediate_with_capabilities(
                                &remote,
                                method,
                                params,
                                &caps,
                                messages,
                                &mut full,
                                512,
                                Default::default(),
                                &mut [0; 8],
                                (),
                                |_, _, _| u8::MAX
                            )
                            .is_err());
                        let mut held = [None, None, None, None, None, None];
                        for slot in &mut held {
                            *slot = Some(messages.reserve(1024).unwrap());
                        }
                        assert!(driver
                            .try_send_immediate_with_capabilities(
                                &remote,
                                method,
                                params,
                                &caps,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 8],
                                (),
                                |_, _, _| u8::MAX
                            )
                            .is_err());
                        for writer in held.into_iter().flatten() {
                            messages.discard(writer);
                        }
                        assert!(driver
                            .try_send_immediate_with_capabilities(
                                &remote,
                                method,
                                params,
                                &caps,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [],
                                (),
                                |_, _, _| u8::MAX
                            )
                            .is_err());
                        assert!(queue.is_empty());
                        assert_eq!(driver.ledgers.questions.len(), 0);
                        for id in 0..8 {
                            assert!(driver
                                .ledgers
                                .exports
                                .lookup(ExportId::from_wire(id))
                                .is_none());
                        }
                        let key = driver
                            .try_send_immediate_with_capabilities(
                                &remote,
                                method,
                                params,
                                &caps,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 8],
                                (),
                                |_, _, _| u8::MAX,
                            )
                            .unwrap()
                            .unwrap();
                        assert_eq!(driver.runtime.queued(), 0);
                        let mut exported = None;
                        drain_by(&mut queue, messages, 1, |root| {
                            let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                                panic!("Call")
                            };
                            let payload = call.unwrap().get_params().unwrap();
                            let table = payload.get_cap_table().unwrap();
                            assert_eq!(table.len(), 4);
                            let rpc_capnp::cap_descriptor::SenderHosted(first) =
                                table.get(0).which().unwrap()
                            else {
                                panic!("local export")
                            };
                            assert!(matches!(
                                table.get(1).which().unwrap(),
                                rpc_capnp::cap_descriptor::None(())
                            ));
                            assert!(matches!(
                                table.get(2).which().unwrap(),
                                rpc_capnp::cap_descriptor::ReceiverHosted(42)
                            ));
                            assert!(
                                matches!(table.get(3).which().unwrap(), rpc_capnp::cap_descriptor::SenderHosted(id) if id == first)
                            );
                            let list = payload
                                .get_content()
                                .get_as::<capnp::any_pointer_list::Reader>()
                                .unwrap();
                            assert!(list.get(1).is_null());
                            assert_eq!(list.get(3).capability_index().unwrap(), 3);
                            exported = Some(ExportId::from_wire(first));
                        });
                        let exported = exported.unwrap();
                        let mut held = [None, None, None, None, None, None];
                        for slot in &mut held {
                            *slot = Some(messages.reserve(1024).unwrap());
                        }
                        assert!(
                            messages.reserve(1024).is_err(),
                            "all six buffers are reusable before Return"
                        );
                        for writer in held.into_iter().flatten() {
                            messages.discard(writer);
                        }
                        if canceled {
                            driver.cancel(key).unwrap();
                        }
                        let reply = Frame::build(messages, 512, Default::default(), |builder| {
                            let mut returned = builder
                                .try_init_root::<Message>()?
                                .into_field(Message::RETURN)
                                .init()?;
                            returned.scalar(Return::ANSWER_ID).set(key.id().to_wire());
                            returned.scalar(Return::RELEASE_PARAM_CAPS).set(release);
                            let mut payload = returned.field(Return::RESULTS).init()?;
                            payload.field(Payload::CONTENT).set_capability_index(0)?;
                            payload
                                .field(Payload::CAP_TABLE)
                                .init(1)?
                                .get_mut(0)
                                .unwrap()
                                .scalar(CapDescriptor::RECEIVER_HOSTED)
                                .set(exported.to_wire());
                            Ok(())
                        })
                        .unwrap();
                        driver
                            .receive(reply, messages, &mut [0; 8], |_| (), || 0)
                            .unwrap_or_else(|rejected| panic!("{}", rejected.error));
                        if canceled {
                            assert!(driver.drain_canceled_response(messages));
                        } else {
                            let response = driver.take_response(key).unwrap();
                            response
                                .with(messages, |reply| {
                                    let super::super::super::Reply::Results(payload) = reply else {
                                        panic!("Results")
                                    };
                                    let Some(reception::full::Capability::Local(lease)) =
                                        payload.capability_at(0).unwrap()
                                    else {
                                        panic!("local lease before final refund")
                                    };
                                    assert!(matches!(
                                        driver
                                            .runtime
                                            .inspect(driver.ledgers.exports.get(lease))
                                            .unwrap(),
                                        clients::Destination::Endpoint {
                                            owner: binding::Route::Capability(
                                                binding::Endpoint::Local(73)
                                            ),
                                            ..
                                        }
                                    ));
                                })
                                .unwrap();
                            driver.release_response(response, messages);
                        }
                        if !release {
                            driver
                                .ledgers
                                .exports
                                .release_deferred(exported, 2)
                                .unwrap();
                        }
                        assert!(driver.ledgers.exports.lookup(exported).is_none());
                        driver.runtime.release(local);
                        driver.runtime.release(remote);
                        close(driver, messages, &mut queue);
                    });
                }
            }
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn generic_calls_keep_exact_grant_metadata_without_allocating_or_pinning_frames() {
    let measured = allocation_counter::measure(|| {
        for native in [false, true] {
            for grants in 0..=4 {
                for repeated in [false, true] {
                    for canceled in [false, true] {
                        for release in [false, true] {
                            with_driver(|mut driver, messages| {
                                if native {
                                    driver.use_native_writer();
                                }
                                let imported =
                                    driver.admit_import(ImportId::from_wire(42), ()).unwrap();
                                let remote = driver.bind_import(&imported).unwrap();
                                driver.release_import(imported);
                                let local = driver
                                    .runtime
                                    .endpoint(binding::Route::local(73), clients::Location::Local)
                                    .unwrap_or_else(|_| panic!("local"));
                                let other = driver
                                    .runtime
                                    .endpoint(binding::Route::local(74), clients::Location::Local)
                                    .unwrap_or_else(|_| panic!("other local"));
                                let caps: [_; 4] = core::array::from_fn(|index| {
                                    Some(if index >= grants {
                                        &remote
                                    } else if repeated || index % 2 == 0 {
                                        &local
                                    } else {
                                        &other
                                    })
                                });
                                let task = driver
                                    .enqueue_dynamic(
                                        &remote,
                                        calls::DynamicCall {
                                            interface: 7,
                                            method: 9,
                                            ..Default::default()
                                        },
                                        &caps,
                                        messages,
                                        512,
                                        Default::default(),
                                        |content| content.set_capability_index(0),
                                    )
                                    .unwrap();
                                let mut blocked = messages::Queue::<()>::new(&mut []).unwrap();
                                assert!(driver
                                    .send_invocation(
                                        messages,
                                        &mut blocked,
                                        512,
                                        Default::default(),
                                        &mut [0; 8]
                                    )
                                    .is_err());
                                let mut slots = [None];
                                let mut queue = messages::Queue::<()>::new(&mut slots).unwrap();
                                // A failed grant check must roll back before the same
                                // invocation can publish its exact metadata on retry.
                                assert!(driver
                                    .send_invocation(
                                        messages,
                                        &mut queue,
                                        512,
                                        Default::default(),
                                        &mut []
                                    )
                                    .is_err());
                                let sent = driver
                                    .send_invocation(
                                        messages,
                                        &mut queue,
                                        512,
                                        Default::default(),
                                        &mut [0; 8],
                                    )
                                    .unwrap()
                                    .unwrap();
                                assert_eq!(sent.task, task);
                                let mut exports = [None; 4];
                                drain_by(&mut queue, messages, 1, |root| {
                                    let rpc_capnp::message::Call(call) = root.which().unwrap()
                                    else {
                                        panic!("Call")
                                    };
                                    let call = call.unwrap();
                                    assert_eq!(
                                        (call.get_interface_id(), call.get_method_id()),
                                        (7, 9)
                                    );
                                    for (index, descriptor) in call
                                        .get_params()
                                        .unwrap()
                                        .get_cap_table()
                                        .unwrap()
                                        .iter()
                                        .enumerate()
                                    {
                                        if let rpc_capnp::cap_descriptor::SenderHosted(id) =
                                            descriptor.which().unwrap()
                                        {
                                            exports[index] = Some(id);
                                        }
                                    }
                                });
                                assert_eq!(exports.iter().flatten().count(), grants);
                                // Six fixed message cells are available, independent
                                // of how many grants the pending question retains.
                                let mut held = [None, None, None, None, None, None];
                                for slot in &mut held {
                                    *slot = Some(messages.reserve(1024).unwrap());
                                }
                                assert!(messages.reserve(1024).is_err());
                                for writer in held.into_iter().flatten() {
                                    messages.discard(writer);
                                }
                                if canceled {
                                    driver.cancel(sent.question).unwrap();
                                }
                                let wrong =
                                    Frame::build(messages, 128, Default::default(), |builder| {
                                        calls::acknowledge(
                                            builder,
                                            AnswerId::from_wire(sent.question.id().to_wire() + 1),
                                        )
                                    })
                                    .unwrap();
                                let rejected = driver
                                    .receive(wrong, messages, &mut [0; 8], |_| (), || 0)
                                    .err()
                                    .expect("unknown Return");
                                rejected.frame.release(messages);
                                for id in exports.into_iter().flatten() {
                                    assert!(driver
                                        .ledgers
                                        .exports
                                        .lookup(ExportId::from_wire(id))
                                        .is_some());
                                }
                                let reply =
                                    Frame::build(messages, 128, Default::default(), |builder| {
                                        let mut returned = builder
                                            .try_init_root::<Message>()?
                                            .into_field(Message::RETURN)
                                            .init()?;
                                        returned
                                            .scalar(Return::ANSWER_ID)
                                            .set(sent.question.id().to_wire());
                                        returned.scalar(Return::RELEASE_PARAM_CAPS).set(release);
                                        returned.scalar(Return::NO_FINISH_NEEDED).set(true);
                                        returned.field(Return::RESULTS).init()?;
                                        Ok(())
                                    })
                                    .unwrap();
                                driver
                                    .receive(reply, messages, &mut [0; 8], |_| (), || 0)
                                    .unwrap_or_else(|rejected| panic!("{}", rejected.error));
                                if canceled {
                                    assert!(driver.drain_canceled_response(messages));
                                } else {
                                    let response = driver.take_response(sent.question).unwrap();
                                    driver.release_response(response, messages);
                                }
                                for id in exports.into_iter().flatten() {
                                    if !release {
                                        driver
                                            .ledgers
                                            .exports
                                            .release_deferred(ExportId::from_wire(id), 1)
                                            .unwrap();
                                    }
                                }
                                for id in exports.into_iter().flatten() {
                                    assert!(
                                        driver
                                            .ledgers
                                            .exports
                                            .lookup(ExportId::from_wire(id))
                                            .is_none(),
                                        "refund every staged grant exactly once"
                                    );
                                }
                                driver.runtime.release(local);
                                driver.runtime.release(other);
                                driver.runtime.release(remote);
                                close(driver, messages, &mut queue);
                            });
                        }
                    }
                }
            }
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn mixed_reflected_descriptors_preserve_routes_and_retry_without_allocating() {
    let measured = allocation_counter::measure(|| {
        for native in [false, true] {
            for reply in [false, true] {
                with_driver(|mut driver, messages| {
                    if native {
                        driver.use_native_writer();
                    }
                    let (call, local) = request(&mut driver, messages, false);
                    let imported = driver.admit_import(ImportId::from_wire(42), ()).unwrap();
                    let remote = driver.bind_import(&imported).unwrap();
                    driver.release_import(imported);
                    let mut slots = [None];
                    let mut queue = messages::Queue::<()>::new(&mut slots).unwrap();
                    let parent = driver
                        .send(
                            messages,
                            &mut queue,
                            128,
                            Default::default(),
                            calls::bootstrap,
                        )
                        .unwrap();
                    drain(&mut queue, messages, |_| ());
                    let projected = driver
                        .pipeline(parent, pipeline::Path::new(&[0, 2]))
                        .unwrap();
                    let caps = [Some(&local), Some(&remote), Some(&projected), None];
                    let mut blocked = messages::Queue::<()>::new(&mut []).unwrap();
                    if reply {
                        let result = driver
                            .local_answer_dynamic(
                                call.answer(),
                                7,
                                9,
                                caps.into_iter(),
                                messages,
                                512,
                                Default::default(),
                                |content| content.set_capability_index(0),
                            )
                            .unwrap();
                        driver
                            .complete_answer(&call, Ok(result), messages)
                            .unwrap_or_else(|_| panic!("completion"));
                        assert!(driver
                            .send_answer(
                                call.answer(),
                                messages,
                                &mut blocked,
                                512,
                                Default::default(),
                                &mut [0; 8],
                                |_, _| Ok(())
                            )
                            .is_err());
                        driver
                            .send_answer(
                                call.answer(),
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 8],
                                |_, _| Ok(()),
                            )
                            .unwrap();
                    } else {
                        driver
                            .enqueue_dynamic(
                                &remote,
                                calls::DynamicCall::default(),
                                &caps,
                                messages,
                                512,
                                Default::default(),
                                |content| content.set_capability_index(0),
                            )
                            .unwrap();
                        assert!(driver
                            .send_invocation(
                                messages,
                                &mut blocked,
                                512,
                                Default::default(),
                                &mut [0; 8]
                            )
                            .is_err());
                        assert!(driver
                            .send_invocation(
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 8]
                            )
                            .unwrap()
                            .is_some());
                    }
                    drain(&mut queue, messages, |root| {
                        let payload = match root.which().unwrap() {
                            rpc_capnp::message::Call(call) if !reply => {
                                call.unwrap().get_params().unwrap()
                            }
                            rpc_capnp::message::Return(returned) if reply => {
                                let returned = returned.unwrap();
                                assert!(!returned.get_no_finish_needed());
                                let rpc_capnp::return_::Results(payload) =
                                    returned.which().unwrap()
                                else {
                                    panic!("Results")
                                };
                                payload.unwrap()
                            }
                            _ => panic!("publication"),
                        };
                        let caps = payload.get_cap_table().unwrap();
                        assert_eq!(caps.len(), 4);
                        assert!(matches!(
                            caps.get(0).which().unwrap(),
                            rpc_capnp::cap_descriptor::SenderHosted(_)
                        ));
                        assert!(matches!(
                            caps.get(1).which().unwrap(),
                            rpc_capnp::cap_descriptor::ReceiverHosted(42)
                        ));
                        let rpc_capnp::cap_descriptor::ReceiverAnswer(answer) =
                            caps.get(2).which().unwrap()
                        else {
                            panic!("ReceiverAnswer")
                        };
                        let answer = answer.unwrap();
                        assert_eq!(answer.get_question_id(), parent.id().to_wire());
                        let mut fields = [0; 2];
                        assert_eq!(
                            pipeline::Path::decode(answer, &mut fields)
                                .unwrap()
                                .fields(),
                            &[0, 2]
                        );
                        assert!(matches!(
                            caps.get(3).which().unwrap(),
                            rpc_capnp::cap_descriptor::None(())
                        ));
                    });
                    driver.release_call(call, messages);
                    driver.runtime.release(local);
                    driver.runtime.release(remote);
                    driver.runtime.release(projected);
                    close(driver, messages, &mut queue);
                });
            }
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn exported_promise_resolves_to_original_peer_routes_without_allocating() {
    let measured = allocation_counter::measure(|| {
        for native in [false, true] {
            for promised in [false, true] {
                with_driver(|mut driver, messages| {
                    if native {
                        driver.use_native_writer();
                    }
                    let (call, local) = request(&mut driver, messages, false);
                    let mut slots = [None];
                    let mut queue = messages::Queue::<()>::new(&mut slots).unwrap();
                    let parent = driver
                        .send(
                            messages,
                            &mut queue,
                            128,
                            Default::default(),
                            calls::bootstrap,
                        )
                        .unwrap();
                    drain(&mut queue, messages, |_| ());
                    let route = if promised {
                        driver.pipeline(parent, pipeline::Path::new(&[2])).unwrap()
                    } else {
                        let imported = driver.admit_import(ImportId::from_wire(42), ()).unwrap();
                        let remote = driver.bind_import(&imported).unwrap();
                        driver.release_import(imported);
                        remote
                    };
                    let source = driver.runtime.promise(&mut None).unwrap();
                    let result = driver
                        .local_answer_dynamic(
                            call.answer(),
                            7,
                            9,
                            [Some(&source)].into_iter(),
                            messages,
                            512,
                            Default::default(),
                            |content| content.set_capability_index(0),
                        )
                        .unwrap();
                    driver
                        .complete_answer(&call, Ok(result), messages)
                        .unwrap_or_else(|_| panic!("completion"));
                    driver
                        .send_answer(
                            call.answer(),
                            messages,
                            &mut queue,
                            512,
                            Default::default(),
                            &mut [0; 8],
                            |_, _| Ok(()),
                        )
                        .unwrap();
                    let mut export_id = 0;
                    drain(&mut queue, messages, |root| {
                        let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                            panic!("Return")
                        };
                        let rpc_capnp::return_::Results(payload) =
                            returned.unwrap().which().unwrap()
                        else {
                            panic!("Results")
                        };
                        let rpc_capnp::cap_descriptor::SenderPromise(id) = payload
                            .unwrap()
                            .get_cap_table()
                            .unwrap()
                            .get(0)
                            .which()
                            .unwrap()
                        else {
                            panic!("SenderPromise")
                        };
                        export_id = id;
                    });
                    assert!(driver
                        .runtime
                        .resolve(source.key(), &mut Some(route), clients::Source::Direct)
                        .unwrap()
                        .is_none());
                    let mut resolved = false;
                    for _ in 0..256 {
                        let _ = driver
                            .poll_maintenance_with(
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [0; 8],
                                NonZeroUsize::new(1).unwrap(),
                                &mut || {},
                                &mut |_, _, _| panic!("reflection must not duplicate resources"),
                                &mut |_, _, _| Ok(None),
                            )
                            .unwrap();
                        if !queue.is_empty() {
                            drain(&mut queue, messages, |root| {
                                let rpc_capnp::message::Resolve(resolve) = root.which().unwrap()
                                else {
                                    return;
                                };
                                let resolve = resolve.unwrap();
                                assert_eq!(resolve.get_promise_id(), export_id);
                                let rpc_capnp::resolve::Cap(cap) = resolve.which().unwrap() else {
                                    panic!("Cap")
                                };
                                match cap.unwrap().which().unwrap() {
                                    rpc_capnp::cap_descriptor::ReceiverHosted(42) if !promised => {}
                                    rpc_capnp::cap_descriptor::ReceiverAnswer(answer)
                                        if promised =>
                                    {
                                        let answer = answer.unwrap();
                                        assert_eq!(answer.get_question_id(), parent.id().to_wire());
                                        assert_eq!(
                                            pipeline::Path::decode(answer, &mut [0])
                                                .unwrap()
                                                .fields(),
                                            &[2]
                                        );
                                    }
                                    _ => panic!("original reflected route"),
                                }
                                resolved = true;
                            });
                        }
                        if resolved {
                            break;
                        }
                    }
                    assert!(resolved);
                    driver.release_call(call, messages);
                    driver.runtime.release(local);
                    driver.runtime.release(source);
                    close(driver, messages, &mut queue);
                });
            }
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn local_result_iterators_preserve_null_slots_and_capacity_without_allocating() {
    let measured = allocation_counter::measure(|| {
        for native in [false, true] {
            for answer in [false, true] {
                with_driver(|mut driver, messages| {
                    if native {
                        driver.use_native_writer();
                    }
                    let (call, target) = request(&mut driver, messages, false);
                    let visited = core::cell::Cell::new(0);
                    let filled = core::cell::Cell::new(false);
                    let rejected = driver.local_result_dynamic(
                        7,
                        9,
                        (0..5).map(|_| {
                            visited.set(visited.get() + 1);
                            Some(&target)
                        }),
                        messages,
                        512,
                        Default::default(),
                        |_| {
                            filled.set(true);
                            Ok(())
                        },
                    );
                    assert!(matches!(
                        rejected,
                        Err(Error::Input(crate::owned::Error::Capacity))
                    ));
                    assert_eq!(visited.get(), 0, "reject capacity before consuming clients");
                    assert!(!filled.get(), "reject capacity before constructing output");

                    let capabilities = (0..4).map(|index| {
                        visited.set(visited.get() + 1);
                        (index % 2 == 0).then_some(&target)
                    });
                    let fill = |field: capnp::field_api::FixedField<
                        '_,
                        capnp::field_api::Generic<capnp::any_pointer::Owned>,
                    >| field.set_capability_index(2);
                    let result = if answer {
                        driver.local_answer_dynamic(
                            call.answer(),
                            7,
                            9,
                            capabilities,
                            messages,
                            512,
                            Default::default(),
                            fill,
                        )
                    } else {
                        driver.local_result_dynamic(
                            7,
                            9,
                            capabilities,
                            messages,
                            512,
                            Default::default(),
                            fill,
                        )
                    }
                    .unwrap();
                    assert_eq!(visited.get(), 4, "consume the lazy table exactly once");
                    let key = target.key();
                    driver.runtime.release(target);
                    result
                        .with(messages, |payload| {
                            assert_eq!(payload.content().capability_index().unwrap(), 2);
                            assert_eq!(payload.capabilities().len(), 4);
                            for (index, client) in payload.capabilities().iter().enumerate() {
                                assert_eq!(
                                    client.as_ref().map(clients::Client::key),
                                    (index % 2 == 0).then_some(key)
                                );
                            }
                        })
                        .unwrap();
                    result.release(messages, &mut driver.runtime);
                    driver.release_call(call, messages);
                    close(
                        driver,
                        messages,
                        &mut messages::Queue::<()>::new(&mut []).unwrap(),
                    );
                });
            }
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn native_grant_publication_checks_descriptors_and_preserves_failed_output() {
    let measured = allocation_counter::measure(|| {
        for native in [false, true] {
            with_driver(|mut driver, messages| {
                let (call, target) = request(&mut driver, messages, false);
                if native {
                    driver.use_native_writer();
                }
                let options = if native {
                    capnp::message::ReaderOptions {
                        traversal_limit_in_words: Some(1),
                        nesting_limit: 1,
                    }
                } else {
                    Default::default()
                };
                let result = driver
                    .local_answer_dynamic(
                        call.answer(),
                        7,
                        9,
                        [Some(&target)].into_iter(),
                        messages,
                        512,
                        options,
                        |content| content.set_capability_index(0),
                    )
                    .unwrap();
                driver
                    .complete_answer(&call, Ok(result), messages)
                    .unwrap_or_else(|_| panic!("completion"));
                let mut slots = [None];
                let mut queue = messages::Queue::new(&mut slots).unwrap();
                // Grant validation still needs its bounded descriptor scratch.
                // Failure must preserve the owed answer and FIFO.
                assert!(driver
                    .send_answer(
                        call.answer(),
                        messages,
                        &mut queue,
                        512,
                        options,
                        &mut [],
                        |_, _| Ok(())
                    )
                    .is_err());
                assert!(queue.is_empty());
                assert!(driver.ledgers.answers.prepare_return(call.answer()).is_ok());
                assert!(driver
                    .send_answer(
                        call.answer(),
                        messages,
                        &mut queue,
                        512,
                        options,
                        &mut [0; 8],
                        |_, _| Ok(())
                    )
                    .unwrap()
                    .is_none());
                drain(&mut queue, messages, |root| {
                    let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                        panic!("Return")
                    };
                    let rpc_capnp::return_::Results(payload) = returned.unwrap().which().unwrap()
                    else {
                        panic!("Results")
                    };
                    let payload = payload.unwrap();
                    assert_eq!(payload.get_cap_table().unwrap().len(), 1);
                    assert_eq!(payload.get_content().capability_index().unwrap(), 0);
                    payload
                        .get_content()
                        .validate_capability_indices(1)
                        .unwrap();
                });
                driver.release_call(call, messages);
                driver.runtime.release(target);
                close(
                    driver,
                    messages,
                    &mut messages::Queue::<()>::new(&mut []).unwrap(),
                );
            });
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn native_writer_keeps_input_limits_storage_and_capability_authority_checked() {
    let limits = capnp::message::ReaderOptions {
        traversal_limit_in_words: Some(1),
        nesting_limit: 1,
    };
    let measured = allocation_counter::measure(|| {
        for policy in [WriterPolicy::Checked, WriterPolicy::Native] {
            with_driver(|mut driver, messages| {
                assert!(!driver.uses_native_writer());
                if policy == WriterPolicy::Native {
                    driver.use_native_writer();
                }
                let result = driver.local_result_dynamic(
                    7,
                    9,
                    core::iter::empty(),
                    messages,
                    512,
                    limits,
                    |content| {
                        content
                            .cast::<capnp::field_api::Text>()
                            .copy_from("native publication")
                    },
                );
                if policy == WriterPolicy::Checked {
                    assert!(result.is_err());
                } else {
                    let result = result.unwrap();
                    result
                        .with(messages, |payload| {
                            assert_eq!(
                                payload.content().get_as::<capnp::text::Reader>().unwrap(),
                                "native publication"
                            );
                        })
                        .unwrap();
                    // The sender's policy cannot relax the next incoming
                    // frame's independent reader budget, even on this driver.
                    let frame = Frame::new(messages.retain(&result.frame.lease).unwrap(), limits);
                    let rejected = driver
                        .receive(frame, messages, &mut [0; 8], |_| (), || 0)
                        .err()
                        .expect("incoming reader limit must reject");
                    assert!(matches!(
                        rejected.error,
                        Error::Input(super::super::super::Error::Wire(
                            capnp::ErrorKind::MessageTooLarge(_)
                                | capnp::ErrorKind::ReadLimitExceeded
                        ))
                    ));
                    rejected.frame.release(messages);
                    result.release(messages, &mut driver.runtime);
                    let invalid = driver
                        .local_result_dynamic(
                            7,
                            9,
                            core::iter::empty(),
                            messages,
                            512,
                            limits,
                            |content| content.set_capability_index(1),
                        )
                        .unwrap();
                    invalid
                        .with(messages, |payload| {
                            let pointer = payload
                                .content()
                                .get_as::<capnp::wire_capability::Reader<'_, ()>>()
                                .unwrap();
                            assert!(matches!(
                                payload.capability(pointer),
                                Err(super::super::super::Error::Capacity)
                            ));
                        })
                        .unwrap();
                    invalid.release(messages, &mut driver.runtime);
                    assert!(
                        driver
                            .local_result_dynamic(
                                7,
                                9,
                                core::iter::empty(),
                                messages,
                                4096,
                                limits,
                                |_| Ok(())
                            )
                            .is_err(),
                        "native writers still respect fixed buffer capacity"
                    );
                }
                close(
                    driver,
                    messages,
                    &mut messages::Queue::<()>::new(&mut []).unwrap(),
                );
            });
        }
    });
    assert_eq!(measured.count_total, 0);
}

fn deferred_promised_request<'p, 'id>(
    driver: &mut Peer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    parent: answers::Key<'id>,
    question: u32,
) -> answers::Key<'id> {
    driver.defer_call_dispatch(true);
    let frame = Frame::build(messages, 512, Default::default(), |builder| {
        calls::dynamic_call(
            builder,
            QuestionId::from_wire(question),
            calls::Target::Promised {
                question: QuestionId::from_wire(parent.id().to_wire()),
                path: pipeline::Path::new(&[]),
            },
            calls::DynamicCall {
                no_promise_pipelining: true,
                ..Default::default()
            },
            |_| Ok(()),
        )
    })
    .unwrap();
    let Received::DeferredCall(key) = driver
        .receive(frame, messages, &mut [0; 8], |_| (), || 84)
        .unwrap_or_else(|rejected| panic!("{}", rejected.error))
    else {
        panic!("deferred Call")
    };
    key
}

#[test]
fn settled_promised_dispatch_needs_no_observer_and_preserves_early_pipeline_priority() {
    for early in [false, true] {
        let measured = allocation_counter::measure(|| {
            with_driver(|mut driver, messages| {
                let (parent, target) = request(&mut driver, messages, false);
                let alternative = driver
                    .runtime
                    .endpoint(binding::Route::local(92), clients::Location::Local)
                    .unwrap();
                let result = driver
                    .local_result_dynamic(
                        7,
                        9,
                        [Some(&target)].into_iter(),
                        messages,
                        512,
                        Default::default(),
                        |content| content.set_capability_index(0),
                    )
                    .unwrap();
                driver
                    .complete_answer(&parent, Ok(result), messages)
                    .unwrap_or_else(|_| panic!("completion"));
                driver.pipelines = pipelines::Bindings::new(&mut []).unwrap();
                let child = deferred_promised_request(&mut driver, messages, parent.answer(), 4);
                assert!(
                    !driver
                        .ledgers
                        .answers
                        .finish(parent.answer().id(), false)
                        .cancel
                );
                let (running, execute) = driver
                    .dispatch_immediate_call_with(true, messages, |_, _, graph| {
                        if early {
                            graph
                                .retain_key(alternative.key())
                                .map(|client| Some(Ok(client)))
                                .map_err(pipelines::Error::Graph)
                        } else {
                            Ok(None)
                        }
                    })
                    .unwrap()
                    .expect("settled target dispatches without an observer");
                assert!(execute);
                assert_eq!(
                    running.target().key(),
                    if early {
                        alternative.key()
                    } else {
                        target.key()
                    }
                );
                assert_eq!(running.owner().answer(), child);
                assert_eq!(driver.runtime.queued(), 0);
                assert!(driver.call_target.is_none());
                let call = driver.runtime.complete(running);
                driver.release_call(call, messages);
                driver.release_call(parent, messages);
                driver.runtime.release(target);
                driver.runtime.release(alternative);
                close(
                    driver,
                    messages,
                    &mut messages::Queue::<()>::new(&mut []).unwrap(),
                );
            })
        });
        assert_eq!(measured.count_total, 0);
    }
}

#[test]
fn resolved_promised_dispatch_reuses_target_and_preserves_order_and_cleanup() {
    for mode in 0..3 {
        let measured = allocation_counter::measure(|| {
            with_driver(|mut driver, messages| {
                let (parent, target) = request(&mut driver, messages, false);
                let waiting = driver.runtime.promise(&mut None).unwrap();
                driver.pipelines = pipelines::Bindings::new(&mut []).unwrap();
                let child = deferred_promised_request(&mut driver, messages, parent.answer(), 4);
                assert!(driver
                    .dispatch_immediate_call_with(true, messages, |_, _, graph| {
                        graph
                            .retain_key(waiting.key())
                            .map(|client| Some(Ok(client)))
                            .map_err(pipelines::Error::Graph)
                    })
                    .unwrap()
                    .is_none());
                assert!(driver.call_target.is_some());
                assert!(driver
                    .dispatch_immediate_call_with(true, messages, |_, _, _| {
                        panic!("retry must retain its original resolved target")
                    })
                    .unwrap()
                    .is_none());
                if mode == 1 {
                    assert_eq!(driver.retry_call().unwrap(), Some(child));
                    assert!(driver.call_target.is_none());
                    let later =
                        deferred_promised_request(&mut driver, messages, parent.answer(), 5);
                    assert!(driver
                        .dispatch_immediate_call_with(true, messages, |_, _, _| {
                            panic!("an older queued call must defer target resolution")
                        })
                        .unwrap()
                        .is_none());
                    let replacement = driver.runtime.retain_key(target.key()).unwrap();
                    let barrier = driver
                        .runtime
                        .prepare_resolution(
                            waiting.key(),
                            &mut Some(replacement),
                            clients::Source::Direct,
                        )
                        .unwrap()
                        .accept();
                    assert!(barrier.is_none());
                    let ready = driver
                        .runtime
                        .next_ready()
                        .unwrap()
                        .expect("resolved child");
                    let running = ready.accept();
                    assert_eq!(running.target().key(), target.key());
                    let call = driver
                        .runtime
                        .complete(running)
                        .into_call()
                        .unwrap_or_else(|_| panic!("child Call"));
                    assert_eq!(call.answer(), child);
                    driver.release_call(call, messages);
                    let (running, _) = driver
                        .dispatch_immediate_call_with(true, messages, |_, _, graph| {
                            graph
                                .retain_key(target.key())
                                .map(|client| Some(Ok(client)))
                                .map_err(pipelines::Error::Graph)
                        })
                        .unwrap()
                        .expect("later child follows the queued child");
                    assert_eq!(running.owner().answer(), later);
                    let call = driver.runtime.complete(running);
                    driver.release_call(call, messages);
                } else if mode == 2 {
                    assert!(driver.ledgers.answers.finish(child.id(), false).cancel);
                    let mut slots = [None];
                    let mut queue = messages::Queue::<()>::new(&mut slots).unwrap();
                    driver.work_cursor = 11;
                    assert!(matches!(
                        driver
                            .poll_maintenance(
                                messages,
                                &mut queue,
                                128,
                                Default::default(),
                                &mut [0; 8],
                                NonZeroUsize::new(1).unwrap()
                            )
                            .unwrap(),
                        Progress::Producer(84)
                    ));
                    assert!(driver.call.is_none());
                    assert!(
                        driver.call_target.is_none(),
                        "cancellation releases the cached target"
                    );
                    drain(&mut queue, messages, |root| {
                        let rpc_capnp::message::Return(Ok(returned)) = root.which().unwrap() else {
                            panic!("Return")
                        };
                        assert_eq!(returned.get_answer_id(), child.id().to_wire());
                        assert!(matches!(
                            returned.which().unwrap(),
                            rpc_capnp::return_::Canceled(())
                        ));
                    });
                }
                driver.runtime.release(waiting);
                driver.release_call(parent, messages);
                driver.runtime.release(target);
                close(
                    driver,
                    messages,
                    &mut messages::Queue::<()>::new(&mut []).unwrap(),
                );
            })
        });
        assert_eq!(measured.count_total, 0);
    }
}

#[cfg(feature = "alloc")]
#[test]
fn native_prepared_return_publishes_with_independent_receiver_limits() {
    let mut prepared = PreparedReturn::from_template(
        &PreparedReturnTemplate::default(),
        capnp::message::HeapAllocator::new(),
    );
    let mut content = prepared.results();
    for _ in 0..16 {
        content = content
            .initn_as::<capnp::any_pointer_list::Builder>(1)
            .get(0);
    }
    content
        .set_as::<capnp::text::Owned>("native return")
        .unwrap();
    let prepared = prepared.freeze();
    let limits = capnp::message::ReaderOptions {
        nesting_limit: 4,
        ..Default::default()
    };
    let measured = allocation_counter::measure(|| {
        for policy in [WriterPolicy::Checked, WriterPolicy::Native] {
            with_driver(|mut driver, messages| {
                let (call, target) = request(&mut driver, messages, false);
                if policy == WriterPolicy::Native {
                    driver.use_native_writer();
                }
                let result =
                    driver.local_answer_prepared(call.answer(), 7, 9, &prepared, messages, limits);
                if policy == WriterPolicy::Checked {
                    assert!(result.is_err());
                } else {
                    let result = result.unwrap().unwrap();
                    driver
                        .complete_answer(&call, Ok(result), messages)
                        .unwrap_or_else(|_| panic!("completion"));
                    let mut slots = [None];
                    let mut queue = messages::Queue::new(&mut slots).unwrap();
                    assert!(driver
                        .send_answer(
                            call.answer(),
                            messages,
                            &mut queue,
                            512,
                            limits,
                            &mut [0; 8],
                            |_, _| Ok(())
                        )
                        .unwrap()
                        .is_some());
                    // Independently decode those exact output bytes under a
                    // receiver's small depth limit, rather than the sender's
                    // unlimited view of its own immutable local builder.
                    let window = queue.window(messages).unwrap().unwrap();
                    let mut bytes = window.bytes();
                    let reader =
                        capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, limits)
                            .unwrap();
                    let rpc_capnp::message::Return(received) = reader
                        .get_root::<rpc_capnp::message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap()
                    else {
                        panic!("Return")
                    };
                    let rpc_capnp::return_::Results(payload) = received.unwrap().which().unwrap()
                    else {
                        panic!("Results")
                    };
                    assert!(payload
                        .unwrap()
                        .get_content()
                        .validate_capability_indices(0)
                        .is_err());
                    drop(window);
                    drain(&mut queue, messages, |root| {
                        let rpc_capnp::message::Return(received) = root.which().unwrap() else {
                            panic!("Return")
                        };
                        let rpc_capnp::return_::Results(payload) =
                            received.unwrap().which().unwrap()
                        else {
                            panic!("Results")
                        };
                        let mut content = payload.unwrap().get_content();
                        for _ in 0..16 {
                            content = content
                                .get_as::<capnp::any_pointer_list::Reader>()
                                .unwrap()
                                .get(0);
                        }
                        assert_eq!(
                            content.get_as::<capnp::text::Reader>().unwrap(),
                            "native return"
                        );
                    });
                    assert!(queue.is_empty());
                }
                driver.release_call(call, messages);
                driver.runtime.release(target);
                close(
                    driver,
                    messages,
                    &mut messages::Queue::<()>::new(&mut []).unwrap(),
                );
            });
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[cfg(feature = "alloc")]
#[test]
fn direct_prepared_return_preserves_dependencies_and_rolls_back_every_capacity_failure() {
    let mut prepared = PreparedReturn::from_template(
        &PreparedReturnTemplate::default(),
        capnp::message::HeapAllocator::new(),
    );
    prepared
        .results()
        .set_as::<capnp::text::Owned>("direct")
        .unwrap();
    let prepared = prepared.freeze();
    let measured = allocation_counter::measure(|| {
        with_driver(|mut driver, messages| {
            let (call, target) = request(&mut driver, messages, false);
            let key = call.answer();
            let pipeline = driver.ledgers.answers.pipeline(key.id()).unwrap();
            let mut slots = [None];
            let mut queue = messages::Queue::new(&mut slots).unwrap();
            assert!(driver
                .try_send_answer_prepared(
                    &call,
                    &prepared,
                    messages,
                    &mut queue,
                    Default::default(),
                    ()
                )
                .unwrap()
                .is_none());
            assert!(queue.is_empty());
            assert!(driver.ledgers.answers.release_pipeline(pipeline).is_none());

            let mut empty = messages::Queue::new(&mut []).unwrap();
            assert!(driver
                .try_send_answer_prepared(
                    &call,
                    &prepared,
                    messages,
                    &mut empty,
                    Default::default(),
                    ()
                )
                .is_err());
            let mut held = [const { None }; 6];
            for slot in &mut held {
                *slot = messages.reserve(1).ok();
            }
            assert!(driver
                .try_send_answer_prepared(
                    &call,
                    &prepared,
                    messages,
                    &mut queue,
                    Default::default(),
                    ()
                )
                .is_err());
            assert!(queue.is_empty());
            assert!(driver.ledgers.answers.get(key).unwrap().owner.is_pending());
            for writer in held.into_iter().flatten() {
                messages.discard(writer);
            }
            let strict = capnp::message::ReaderOptions {
                nesting_limit: 0,
                ..Default::default()
            };
            assert!(driver
                .try_send_answer_prepared(&call, &prepared, messages, &mut queue, strict, ())
                .is_err());
            assert!(queue.is_empty());
            assert!(driver.ledgers.answers.get(key).unwrap().owner.is_pending());

            assert_eq!(
                driver
                    .try_send_answer_prepared(
                        &call,
                        &prepared,
                        messages,
                        &mut queue,
                        Default::default(),
                        ()
                    )
                    .unwrap(),
                Some(73)
            );
            assert!(driver.ledgers.answers.get(key).is_err());
            drain(&mut queue, messages, |root| {
                let rpc_capnp::message::Return(Ok(returned)) = root.which().unwrap() else {
                    panic!("return")
                };
                assert!(returned.get_no_finish_needed());
                let rpc_capnp::return_::Results(Ok(payload)) = returned.which().unwrap() else {
                    panic!("results")
                };
                assert_eq!(
                    payload
                        .get_content()
                        .get_as::<capnp::text::Reader>()
                        .unwrap(),
                    "direct"
                );
            });
            driver.release_call(call, messages);
            driver.runtime.release(target);
            close(driver, messages, &mut queue);
        })
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn prepared_data_reply_reuses_storage_and_late_cancel_rebuilds_without_allocating() {
    check_prepared_data_reply(false);
}

#[cfg(feature = "alloc")]
#[test]
fn hosted_return_template_preserves_retry_cancellation_and_allocation_free_publication() {
    check_prepared_data_reply(true);
}

fn check_prepared_data_reply(_hosted: bool) {
    // Only the hosted application builder allocates. Its envelope relocation,
    // validation and publication must still use the caller's fixed storage.
    #[cfg(feature = "alloc")]
    let prepared = _hosted.then(|| {
        let mut source = PreparedReturn::from_template(
            &PreparedReturnTemplate::default(),
            capnp::message::HeapAllocator::new(),
        );
        source
            .results()
            .set_as::<capnp::text::Owned>("retained result")
            .unwrap();
        source.freeze()
    });
    for canceled in [false, true] {
        let measured = allocation_counter::measure(|| {
            with_driver(|mut driver, messages| {
                let (call, target) = request(&mut driver, messages, false);
                let key = call.answer();
                #[cfg(feature = "alloc")]
                let result = prepared.as_ref().map(|source| {
                    driver
                        .local_answer_prepared(key, 7, 9, source, messages, Default::default())
                        .unwrap()
                        .unwrap()
                });
                let mut dynamic = || {
                    driver
                        .local_answer_dynamic(
                            key,
                            7,
                            9,
                            core::iter::empty(),
                            messages,
                            512,
                            Default::default(),
                            |field| {
                                field
                                    .cast::<capnp::field_api::Text>()
                                    .copy_from("retained result")
                            },
                        )
                        .unwrap()
                };
                #[cfg(feature = "alloc")]
                let result = result.unwrap_or_else(&mut dynamic);
                #[cfg(not(feature = "alloc"))]
                let result = dynamic();
                let original = messages.read(&result.frame.lease).as_ptr();
                driver
                    .complete_answer(&call, Ok(result), messages)
                    .unwrap_or_else(|_| panic!("completion"));
                if canceled {
                    driver.ledgers.answers.finish(key.id(), false);
                }
                // Every byte buffer is occupied. A data-only prepared reply needs
                // no new buffer; cancellation still needs its distinct envelope.
                let mut held = [const { None }; 6];
                for slot in &mut held {
                    *slot = messages.reserve(1).ok();
                }
                let mut slots = [None];
                let mut queue = messages::Queue::new(&mut slots).unwrap();
                if !canceled {
                    let strict = capnp::message::ReaderOptions {
                        nesting_limit: 0,
                        ..Default::default()
                    };
                    assert!(
                        driver
                            .send_answer(
                                key,
                                messages,
                                &mut queue,
                                512,
                                strict,
                                &mut [0; 8],
                                |_, _| panic!("exception"),
                            )
                            .is_err(),
                        "a certificate does not bypass changed reader limits"
                    );
                    assert!(queue.is_empty());
                    assert!(driver.ledgers.answers.prepare_return(key).is_ok());
                }
                let outcome = driver.send_answer(
                    key,
                    messages,
                    &mut queue,
                    512,
                    Default::default(),
                    &mut [0; 8],
                    |_, _| panic!("exception"),
                );
                if canceled {
                    assert!(outcome.is_err());
                    assert!(queue.is_empty());
                    messages.discard(held.iter_mut().find_map(Option::take).unwrap());
                    driver
                        .send_answer(
                            key,
                            messages,
                            &mut queue,
                            512,
                            Default::default(),
                            &mut [0; 8],
                            |_, _| panic!("exception"),
                        )
                        .unwrap();
                } else {
                    assert!(outcome.is_ok());
                    let window = queue.window(messages).unwrap().unwrap();
                    assert_eq!(window.bytes().as_ptr(), original);
                    drop(window);
                }
                drain(&mut queue, messages, |root| {
                    let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                        panic!("Return");
                    };
                    let returned = returned.unwrap();
                    assert_eq!(returned.get_answer_id(), 3);
                    if canceled {
                        assert!(matches!(
                            returned.which().unwrap(),
                            rpc_capnp::return_::Canceled(())
                        ));
                    } else {
                        assert!(returned.get_no_finish_needed());
                        let rpc_capnp::return_::Results(payload) = returned.which().unwrap() else {
                            panic!("results");
                        };
                        assert_eq!(
                            payload
                                .unwrap()
                                .get_content()
                                .get_as::<capnp::text::Reader>()
                                .unwrap(),
                            "retained result"
                        );
                    }
                });
                for writer in held.into_iter().flatten() {
                    messages.discard(writer);
                }
                driver.release_call(call, messages);
                driver.runtime.release(target);
                close(driver, messages, &mut queue);
            })
        });
        assert_eq!(measured.count_total, 0);
    }
}

#[test]
fn introduction_publication_validates_grants_and_retries_without_allocation() {
    use crate::publication::descriptors::Descriptors;
    use crate::rpc_capnp::api::ThirdPartyCapDescriptor;
    let measured = allocation_counter::measure(|| {
        with_driver(|mut driver, messages| {
            let (call, target) = request(&mut driver, messages, false);
            let vine = driver
                .runtime
                .endpoint(binding::Route::Vine(99), clients::Location::Local)
                .unwrap();
            let result = driver
                .local_result_dynamic(
                    7,
                    9,
                    [Some(&vine)].into_iter(),
                    messages,
                    256,
                    Default::default(),
                    |field| field.set_capability_index(0),
                )
                .unwrap();
            driver
                .complete_answer(&call, Ok(result), messages)
                .unwrap_or_else(|_| panic!("completion"));
            let mut slots = [None];
            let mut queue = messages::Queue::new(&mut slots).unwrap();
            driver.set_descriptors(Descriptors {
                identity: |_| None,
                extra_bytes: |_| Ok(64),
                write: |_, _, mut field| {
                    let mut third = field.field(CapDescriptor::THIRD_PARTY_HOSTED).init()?;
                    third.scalar(ThirdPartyCapDescriptor::VINE_ID).set(u32::MAX);
                    Ok(())
                },
            });
            assert_eq!(
                driver
                    .send_answer(
                        call.answer(),
                        messages,
                        &mut queue,
                        256,
                        Default::default(),
                        &mut [0; 8],
                        |_, _| Ok(())
                    )
                    .unwrap_err(),
                Error::Input(crate::owned::Error::Output(
                    crate::admission::Error::Publication(crate::publication::Error::Descriptors(
                        exports::DescriptorError::Unreserved
                    ))
                ))
            );
            assert!(queue.is_empty());
            driver.set_descriptors(Descriptors {
                identity: |_| None,
                extra_bytes: |_| Ok(64),
                write: |_, id, mut field| {
                    let mut third = field.field(CapDescriptor::THIRD_PARTY_HOSTED).init()?;
                    third
                        .scalar(ThirdPartyCapDescriptor::VINE_ID)
                        .set(id.to_wire());
                    third
                        .field(ThirdPartyCapDescriptor::ID)
                        .cast::<capnp::field_api::Data>()
                        .copy_from(&b"authenticated"[..])
                },
            });
            driver
                .send_answer(
                    call.answer(),
                    messages,
                    &mut queue,
                    256,
                    Default::default(),
                    &mut [0; 8],
                    |_, _| Ok(()),
                )
                .unwrap();
            drain(&mut queue, messages, |root| {
                let rpc_capnp::message::Return(ret) = root.which().unwrap() else {
                    panic!("Return")
                };
                let rpc_capnp::return_::Results(payload) = ret.unwrap().which().unwrap() else {
                    panic!("Results")
                };
                let rpc_capnp::cap_descriptor::ThirdPartyHosted(third) = payload
                    .unwrap()
                    .get_cap_table()
                    .unwrap()
                    .get(0)
                    .which()
                    .unwrap()
                else {
                    panic!("introduction")
                };
                let third = third.unwrap();
                assert_eq!(
                    third.get_id().get_as::<capnp::data::Reader>().unwrap(),
                    b"authenticated"
                );
                assert!(driver
                    .ledgers
                    .exports
                    .lookup(ExportId::from_wire(third.get_vine_id()))
                    .is_some());
            });
            driver.release_call(call, messages);
            driver.runtime.release(vine);
            driver.runtime.release(target);
            close(driver, messages, &mut queue);
        });
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn pipeline_only_sender_exposes_a_settled_route_and_defers_finish_without_allocating() {
    let measured = allocation_counter::measure(|| {
        with_driver(|mut driver, messages| {
            let mut imported = [imports::ReceiveSlot::new(ImportId::from_wire(41), ())];
            driver
                .ledgers
                .imports
                .prepare_receive(&mut imported)
                .unwrap()
                .commit();
            let receipt = imported[0].take_local().unwrap();
            let target = driver
                .imports
                .bind(&mut driver.runtime, &mut driver.ledgers.imports, &receipt)
                .unwrap();
            driver.ledgers.imports.release(receipt);
            driver
                .enqueue_dynamic(
                    &target,
                    calls::DynamicCall {
                        only_promise_pipeline: true,
                        ..Default::default()
                    },
                    &[],
                    messages,
                    256,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap();
            let mut slots = [None];
            let mut queue = messages::Queue::new(&mut slots).unwrap();
            let key = driver
                .send_invocation(messages, &mut queue, 256, Default::default(), &mut [0; 8])
                .unwrap()
                .unwrap()
                .question;
            assert!(key.id().is_pipeline_only());
            drain(&mut queue, messages, |root| {
                let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                    panic!("Call")
                };
                assert!(call.unwrap().get_only_promise_pipeline());
            });
            let projected = driver.pipeline(key, pipeline::Path::new(&[0])).unwrap();
            assert!(
                driver.runtime.is_settled(&projected).unwrap(),
                "a no-Return pipeline must never wait for Return resolution"
            );
            let clients::Destination::Endpoint {
                owner: binding::Route::Question(route),
                ..
            } = driver.runtime.inspect(&projected).unwrap()
            else {
                panic!("question route")
            };
            let (id, path) = route.target(&driver.ledgers.questions).unwrap();
            assert_eq!(id, key.id());
            assert_eq!(path.fields(), &[0]);
            driver.cancel(key).unwrap();
            assert!(driver.drain_canceled_response(messages));
            assert!(
                !driver
                    .send_pending_finish(messages, &mut queue, 128, Default::default(),)
                    .unwrap(),
                "a live pipeline still owns the high question"
            );
            driver.runtime.release(projected);
            assert!(driver.drain_endpoint().is_some());
            assert!(driver
                .send_pending_finish(messages, &mut queue, 128, Default::default(),)
                .unwrap());
            drain(&mut queue, messages, |root| {
                let rpc_capnp::message::Finish(finish) = root.which().unwrap() else {
                    panic!("Finish")
                };
                let finish = finish.unwrap();
                assert_eq!(finish.get_question_id(), key.id().to_wire());
                assert!(finish.get_release_result_caps());
            });
            assert!(driver.ledgers.questions.is_empty());
            driver.runtime.release(target);
            close(driver, messages, &mut queue);
        });
    });
    assert_eq!(measured.count_total, 0);
}

fn with_driver(
    f: impl for<'p, 'id, 's, 'm, 'b> FnOnce(Peer<'p, 'id, 's>, &mut messages::Messages<'p, 'm, 'b>),
) {
    let mut bytes = [[ZERO; 128]; 6];
    let mut buffers = bytes
        .each_mut()
        .map(|b| messages::Buffer::new(Word::words_to_bytes_mut(b)));
    messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        storage::with_domain(|domain| {
            let mut slots = FixedStorage::<(), u32, u8, u32, 4, 4, 8, 24>::new();
            f(slots.bind(domain, 1).unwrap(), messages);
        });
    })
    .unwrap();
}
fn request<'p, 'id>(
    driver: &mut Peer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    redirected: bool,
) -> (Request<'p, 'id, (), 4, 4>, clients::Client<'id>) {
    request_with_hint(driver, messages, redirected, false)
}
fn request_with_hint<'p, 'id>(
    driver: &mut Peer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    redirected: bool,
    only_pipeline: bool,
) -> (Request<'p, 'id, (), 4, 4>, clients::Client<'id>) {
    let target = driver
        .runtime
        .endpoint(binding::Route::local(91), clients::Location::Local)
        .unwrap();
    let id = {
        let mut batch = driver.ledgers.exports.batch();
        let id = binding::stage(&mut driver.runtime, &mut batch, &target)
            .unwrap()
            .id();
        batch.commit();
        id
    };
    let frame = Frame::build(messages, 512, Default::default(), |builder| {
        calls::dynamic_call(
            builder,
            QuestionId::from_wire(3),
            calls::Target::Imported(ImportId::from_wire(id.to_wire())),
            calls::DynamicCall {
                interface: 7,
                method: 9,
                only_promise_pipeline: only_pipeline,
                ..Default::default()
            },
            |_| Ok(()),
        )?;
        if redirected {
            use crate::rpc_capnp::api::{call::SendResultsTo, Call};
            builder
                .try_get_root::<Message>()?
                .into_field(Message::CALL)
                .edit()?
                .group(Call::SEND_RESULTS_TO)
                .select()
                .void(SendResultsTo::YOURSELF)
                .set();
        }
        Ok(())
    })
    .unwrap();
    let call = Request::admit(
        frame,
        messages,
        &mut driver.ledgers.imports,
        &mut driver.ledgers.exports,
        &mut driver.ledgers.answers,
        Answer::new(pipelines::Results::new(73)),
        |_| (),
    )
    .unwrap_or_else(|_| panic!("admission"));
    assert!(driver
        .ledgers
        .answers
        .dispatch(call.answer(), true)
        .unwrap());
    (call, target)
}

#[test]
fn pipeline_only_completion_retains_capabilities_until_finish_without_allocating() {
    let measured = allocation_counter::measure(|| {
        for early_finish in [false, true] {
            with_driver(|mut driver, messages| {
                let (call, target) = request_with_hint(&mut driver, messages, false, true);
                let key = call.answer();
                let lease = driver.ledgers.answers.pipeline(key.id()).unwrap();
                if early_finish {
                    assert!(driver
                        .ledgers
                        .answers
                        .finish(key.id(), true)
                        .retired
                        .is_none());
                }
                let result = driver
                    .local_result_dynamic(
                        7,
                        9,
                        [Some(&target)].into_iter(),
                        messages,
                        512,
                        Default::default(),
                        |field| field.set_capability_index(0),
                    )
                    .unwrap();
                driver
                    .complete_answer(&call, Ok(result), messages)
                    .unwrap_or_else(|_| panic!("completion"));
                assert!(driver
                    .send_pipeline_only_answer(&call, messages)
                    .unwrap()
                    .is_none());
                assert!(
                    driver.send_pipeline_only_answer(&call, messages).is_err(),
                    "publication is once-only"
                );
                if !early_finish {
                    let finish = Frame::build(messages, 128, Default::default(), |builder| {
                        use crate::rpc_capnp::api::Finish;
                        let root = builder.try_init_root::<Message>()?;
                        let mut finish = root.into_field(Message::FINISH).init()?;
                        finish.scalar(Finish::QUESTION_ID).set(key.id().to_wire());
                        finish.scalar(Finish::RELEASE_RESULT_CAPS).set(true);
                        Ok(())
                    })
                    .unwrap();
                    assert!(matches!(
                        driver
                            .receive(finish, messages, &mut [0; 8], |_| (), || 0)
                            .unwrap_or_else(|r| panic!("{:?}", r.error)),
                        Received::Finished(None)
                    ));
                }
                let projected = driver
                    .ledgers
                    .answers
                    .get(key)
                    .unwrap()
                    .owner
                    .target(pipeline::Path::new(&[]), messages, &mut driver.runtime)
                    .unwrap()
                    .unwrap();
                assert_eq!(projected.key(), target.key());
                driver.runtime.release(projected);
                driver.release_call(call, messages);
                let retired = driver
                    .release_answer_pipeline(lease)
                    .expect("last pipeline retires answer");
                assert_eq!(driver.release_answer(retired, messages), 73);
                assert!(driver.ledgers.answers.is_empty());
                driver.runtime.release(target);
                close(
                    driver,
                    messages,
                    &mut messages::Queue::<()>::new(&mut []).unwrap(),
                );
            });
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn ordinary_and_redirected_calls_cannot_skip_return_publication() {
    for (redirected, only_pipeline) in [(false, false), (true, true)] {
        with_driver(|mut driver, messages| {
            let (call, target) =
                request_with_hint(&mut driver, messages, redirected, only_pipeline);
            let result = driver
                .local_result_dynamic(
                    7,
                    9,
                    [].into_iter(),
                    messages,
                    256,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap();
            driver
                .complete_answer(&call, Ok(result), messages)
                .unwrap_or_else(|_| panic!("completion"));
            assert!(driver.send_pipeline_only_answer(&call, messages).is_err());
            let mut slots = [None];
            let mut queue = messages::Queue::<()>::new(&mut slots).unwrap();
            driver
                .send_answer(
                    call.answer(),
                    messages,
                    &mut queue,
                    256,
                    Default::default(),
                    &mut [0; 8],
                    |_, _| Ok(()),
                )
                .unwrap();
            assert!(
                !queue.is_empty(),
                "ordinary Return remains owed after rejection"
            );
            driver.release_call(call, messages);
            driver.runtime.release(target);
            close(driver, messages, &mut queue);
        });
    }
}
fn drain<'p, R>(
    queue: &mut messages::Queue<'p, '_, R>,
    messages: &mut messages::Messages<'p, '_, '_>,
    check: impl FnOnce(rpc_capnp::message::Reader<'_>),
) {
    drain_by(queue, messages, usize::MAX, check);
}
fn drain_by<'p, R>(
    queue: &mut messages::Queue<'p, '_, R>,
    messages: &mut messages::Messages<'p, '_, '_>,
    chunk: usize,
    check: impl FnOnce(rpc_capnp::message::Reader<'_>),
) {
    let window = queue.window(messages).unwrap().unwrap();
    let mut bytes = window.bytes();
    let parsed =
        capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, Default::default())
            .unwrap();
    check(parsed.get_root().unwrap());
    let n = window.bytes().len();
    window.advance(n.min(chunk)).unwrap();
    while let Some(window) = queue.window(messages).unwrap() {
        let n = window.bytes().len();
        window.advance(n.min(chunk)).unwrap();
    }
}
fn close<'p, R>(
    driver: Peer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_, R>,
) {
    let mut terminal = driver.terminate(queue);
    for _ in 0..256 {
        if matches!(
            terminal
                .poll(messages, NonZeroUsize::new(32).unwrap())
                .unwrap(),
            Termination::Done
        ) {
            let mut slots = [None, None, None, None, None, None];
            for s in &mut slots {
                *s = Some(messages.reserve(1024).unwrap());
            }
            for s in slots {
                messages.discard(s.unwrap());
            }
            return;
        }
    }
    panic!("cleanup");
}
#[test]
fn adopted_output_waits_for_local_completion_and_retains_result_after_finish_without_allocating() {
    for failed in [false, true] {
        let measured = allocation_counter::measure(|| {
            with_driver(|mut driver, messages| {
                let (call, target) = request(&mut driver, messages, true);
                let mut slots = [None];
                let mut queue = messages::Queue::new(&mut slots).unwrap();
                let question = driver
                    .send(
                        messages,
                        &mut queue,
                        512,
                        Default::default(),
                        |builder, id| {
                            calls::dynamic_call(
                                builder,
                                id,
                                calls::Target::Imported(ImportId::from_wire(0)),
                                calls::DynamicCall {
                                    interface: 99,
                                    method: 12,
                                    ..Default::default()
                                },
                                |_| Ok(()),
                            )
                        },
                    )
                    .unwrap();
                drain(&mut queue, messages, |_| ());
                let returned = Frame::build(messages, 512, Default::default(), |builder| {
                    let mut root = builder.try_init_root::<Message>()?;
                    let mut returned = root.field(Message::RETURN).init()?;
                    returned
                        .scalar(Return::ANSWER_ID)
                        .set(question.id().to_wire());
                    returned
                        .scalar(Return::TAKE_FROM_OTHER_QUESTION)
                        .set(call.answer().id().to_wire());
                    Ok(())
                })
                .unwrap();
                driver
                    .receive(returned, messages, &mut [0; 8], |_| (), || 0)
                    .unwrap_or_else(|_| panic!("adopted return"));
                assert!(
                    matches!(
                        driver.take_output(question, messages),
                        Err(Error::PendingResponse)
                    ),
                    "adoption is not an application response before local completion"
                );
                let result = if failed {
                    Err(capnp::ErrorKind::Overloaded)
                } else {
                    Ok(driver
                        .local_result_dynamic(
                            7,
                            9,
                            [None, Some(&target)].into_iter(),
                            messages,
                            512,
                            Default::default(),
                            |field| field.set_capability_index(1),
                        )
                        .unwrap())
                };
                driver
                    .complete_answer(&call, result, messages)
                    .unwrap_or_else(|_| panic!("completed"));
                // Finish and release the original invocation before consuming
                // the result. The adoption lease alone must preserve authority.
                assert!(driver
                    .ledgers
                    .answers
                    .finish(call.answer().id(), false)
                    .retired
                    .is_none());
                driver.release_call(call, messages);
                let output = driver.take_output(question, messages).unwrap();
                if failed {
                    assert!(matches!(output, Err(capnp::ErrorKind::Overloaded)));
                } else {
                    let Ok(CallOutput::Local(result)) = output else {
                        panic!("local adopted payload")
                    };
                    result
                        .with(messages, |payload| {
                            assert_eq!((payload.interface, payload.method), (99, 12));
                            assert_eq!(payload.capabilities().len(), 2);
                            assert!(payload.capabilities()[0].is_none());
                            assert_eq!(payload.content().capability_index().unwrap(), 1);
                            assert_eq!(
                                payload.capabilities()[1].as_ref().unwrap().key(),
                                target.key()
                            );
                        })
                        .unwrap();
                    result.release(messages, &mut driver.runtime);
                }
                assert!(
                    driver.take_output(question, messages).is_err(),
                    "result is consumed once"
                );
                driver.runtime.release(target);
                close(driver, messages, &mut queue);
            });
        });
        assert_eq!(measured.count_total, 0);
    }
}

#[test]
fn completed_answer_retries_without_allocating_and_keeps_pipeline_after_finish() {
    for cancel in [false, true] {
        for redirected in [false, true] {
            let measured = allocation_counter::measure(|| {
                with_driver(|mut driver, messages| {
                    let (call, target) = request(&mut driver, messages, redirected);
                    let key = call.answer();
                    let parent = driver.ledgers.answers.pipeline(key.id()).unwrap();
                    let observer = driver
                        .pipelines
                        .bind(
                            &mut driver.runtime,
                            &mut driver.ledgers.answers,
                            &parent,
                            pipeline::Path::new(&[]),
                        )
                        .unwrap();
                    driver.ledgers.answers.release_pipeline_deferred(parent);
                    let result = driver
                        .local_result_dynamic(
                            7,
                            9,
                            [None, Some(&target)].into_iter(),
                            messages,
                            512,
                            Default::default(),
                            |p| p.set_capability_index(1),
                        )
                        .unwrap();
                    driver
                        .complete_answer(&call, Ok(result), messages)
                        .unwrap_or_else(|_| panic!("completion"));
                    let rejected = driver
                        .complete_answer(&call, Err(capnp::ErrorKind::Failed), messages)
                        .err()
                        .unwrap();
                    assert_eq!(
                        rejected.error,
                        Error::Pipeline(pipelines::Error::AlreadyCompleted)
                    );
                    let mut slots = [None];
                    let mut queue =
                        messages::Queue::<crate::attachments::Sender<u32, 2>>::with_resources(
                            &mut slots,
                        )
                        .unwrap();
                    let blocker = Frame::build(messages, 512, Default::default(), |b| {
                        calls::acknowledge(b, capntproto_rpc_engine::ids::AnswerId::from_wire(99))
                    })
                    .unwrap();
                    let offer = messages.offer(&blocker.lease).unwrap();
                    queue.reserve().unwrap().accept(messages, offer);
                    blocker.release(messages);
                    assert!(driver
                        .send_answer(
                            key,
                            messages,
                            &mut queue,
                            512,
                            Default::default(),
                            &mut [0; 8],
                            |_, _| Ok(())
                        )
                        .is_err());
                    assert!(driver
                        .ledgers
                        .answers
                        .get(key)
                        .unwrap()
                        .with_result(messages, |_| ())
                        .unwrap()
                        .is_none());
                    drain(&mut queue, messages, |_| ());
                    // Failure after grants were staged must preserve the result and
                    // leave Return owed, just like failure before reservation.
                    if !redirected {
                        assert!(driver
                            .send_answer(
                                key,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [],
                                |_, _| Ok(())
                            )
                            .is_err());
                    }
                    if cancel {
                        assert!(driver
                            .ledgers
                            .answers
                            .finish(key.id(), false)
                            .retired
                            .is_none());
                    }
                    assert!(driver
                        .send_answer_with_attachments(
                            key,
                            messages,
                            &mut queue,
                            512,
                            Default::default(),
                            &mut [0; 8],
                            Capacity::Inline,
                            |_| None,
                            |owner| Some(*owner),
                            |_, _| Ok(())
                        )
                        .unwrap()
                        .is_none());
                    {
                        let window = queue.window(messages).unwrap().unwrap();
                        let resources = window.resources().unwrap();
                        assert_eq!(resources.len(), usize::from(!cancel && !redirected));
                        if !cancel && !redirected {
                            assert_eq!(resources.iter().copied().next(), Some(91));
                        }
                    }
                    drain(&mut queue, messages, |root| {
                        let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                            panic!("return")
                        };
                        let returned = returned.unwrap();
                        assert_eq!(returned.get_answer_id(), 3);
                        match returned.which().unwrap() {
                            rpc_capnp::return_::Canceled(()) => assert!(cancel),
                            rpc_capnp::return_::ResultsSentElsewhere(()) => {
                                assert!(redirected && !cancel)
                            }
                            rpc_capnp::return_::Results(payload) => {
                                assert!(!cancel && !redirected);
                                let payload = payload.unwrap();
                                assert_eq!(payload.get_content().capability_index().unwrap(), 1);
                                let caps = payload.get_cap_table().unwrap();
                                assert_eq!(caps.len(), 2);
                                assert!(matches!(
                                    caps.get(0).which().unwrap(),
                                    rpc_capnp::cap_descriptor::None(())
                                ));
                                assert_eq!(caps.get(1).get_attached_fd(), 0);
                            }
                            _ => panic!("wrong return"),
                        }
                    });
                    assert_eq!(
                        driver.poll_pipeline(0, messages).unwrap(),
                        Propagation::Resolved
                    );
                    assert!(matches!(
                        driver.runtime.inspect(&observer).unwrap(),
                        clients::Destination::Endpoint {
                            owner: binding::Route::Capability(binding::Endpoint::Local(91)),
                            ..
                        }
                    ));
                    driver.runtime.release(observer);
                    driver.release_call(call, messages);
                    driver.runtime.release(target);
                    close(driver, messages, &mut queue);
                })
            });
            assert_eq!(measured.count_total, 0);
        }
    }
}

#[test]
fn wrong_method_and_exception_encoder_failure_preserve_completion() {
    with_driver(|mut driver, messages| {
        let (call, target) = request(&mut driver, messages, false);
        let key = call.answer();
        let result = driver
            .local_result_dynamic(
                7,
                10,
                core::iter::empty(),
                messages,
                512,
                Default::default(),
                |_| Ok(()),
            )
            .unwrap();
        let rejected = driver
            .complete_answer(&call, Ok(result), messages)
            .err()
            .unwrap();
        assert_eq!(
            rejected.error,
            Error::Input(crate::owned::Error::WrongMessage)
        );
        rejected
            .result
            .unwrap_or_else(|_| panic!("preserved result"))
            .release(messages, &mut driver.runtime);
        driver
            .complete_answer(&call, Err(capnp::ErrorKind::Overloaded), messages)
            .unwrap_or_else(|_| panic!("completion"));
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        assert!(driver
            .send_answer(
                key,
                messages,
                &mut queue,
                512,
                Default::default(),
                &mut [],
                |_, _| Err(capnp::Error::from_kind(capnp::ErrorKind::Failed))
            )
            .is_err());
        assert!(queue.is_empty());
        driver
            .send_answer(
                key,
                messages,
                &mut queue,
                512,
                Default::default(),
                &mut [],
                |producer, mut exception| {
                    assert_eq!(*producer, 73);
                    exception.field(Exception::REASON).copy_from("retry me")
                },
            )
            .unwrap();
        drain(&mut queue, messages, |root| {
            let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                panic!()
            };
            let rpc_capnp::return_::Exception(exception) = returned.unwrap().which().unwrap()
            else {
                panic!()
            };
            let exception = exception.unwrap();
            assert_eq!(
                exception.get_type().unwrap(),
                rpc_capnp::exception::Type::Overloaded
            );
            assert_eq!(
                exception.get_reason().unwrap().to_str().unwrap(),
                "retry me"
            );
        });
        driver.release_call(call, messages);
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn early_pipeline_retry_is_transactional_and_survives_final_failure() {
    with_driver(|mut driver, messages| {
        let (call, target) = request(&mut driver, messages, false);
        let parent = driver.ledgers.answers.pipeline(call.answer().id()).unwrap();
        let observer = driver
            .pipelines
            .bind(
                &mut driver.runtime,
                &mut driver.ledgers.answers,
                &parent,
                pipeline::Path::new(&[2, 3]),
            )
            .unwrap();
        driver.ledgers.answers.release_pipeline_deferred(parent);
        assert!(driver
            .poll_pipeline_with(0, messages, |_, _, _| Err(pipelines::Error::Capacity))
            .is_err());
        assert_eq!(
            driver
                .poll_pipeline_with(0, messages, |_, _, _| Ok(None))
                .unwrap(),
            Propagation::Pending
        );
        driver
            .complete_answer(&call, Err(capnp::ErrorKind::Overloaded), messages)
            .unwrap_or_else(|_| panic!("completion"));
        assert_eq!(
            driver
                .poll_pipeline_with(0, messages, |producer, path, graph| {
                    assert_eq!(*producer, 73);
                    assert_eq!(path.fields(), [2, 3]);
                    graph
                        .retain(&target)
                        .map(|c| Some(Ok(c)))
                        .map_err(pipelines::Error::Graph)
                })
                .unwrap(),
            Propagation::Resolved
        );
        assert!(matches!(
            driver.runtime.inspect(&observer).unwrap(),
            clients::Destination::Endpoint {
                location: clients::Location::Local,
                ..
            }
        ));
        driver.runtime.release(observer);
        driver.release_call(call, messages);
        driver.runtime.release(target);
        let mut slots = [None];
        close(
            driver,
            messages,
            &mut messages::Queue::new(&mut slots).unwrap(),
        );
    });
}

#[test]
fn maintenance_selection_skips_later_compound_readiness_checks() {
    with_driver(|mut driver, messages| {
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        let client = driver
            .runtime
            .endpoint(
                binding::Route::Capability(crate::clients::Endpoint::Broken(
                    capnp::ErrorKind::Failed,
                )),
                clients::Location::Broken,
            )
            .unwrap();
        driver.runtime.release(client);
        driver.work_cursor = 5;
        driver.readiness_visited.set(0);
        assert!(!driver.maintenance_is_idle(messages, &queue));
        assert_eq!(
            driver.readiness_visited.get(),
            0,
            "selection must not inspect later compound producers"
        );
        assert_eq!(driver.work_cursor, 5, "idle inspection preserves fairness");
        driver.work_cursor = 0;
        driver.readiness_visited.set(0);
        assert!(!driver.maintenance_is_idle(messages, &queue));
        assert_eq!(
            driver.readiness_visited.get(),
            (1 << 2) | (1 << 3),
            "only compound producers preceding retirement need inspection"
        );

        close(driver, messages, &mut queue);
    });
}

#[test]
fn endpoint_retirement_charges_each_owner_and_preserves_fair_selection() {
    with_driver(|mut driver, messages| {
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        for _ in 0..10 {
            let client = driver
                .runtime
                .endpoint(
                    binding::Route::Capability(crate::clients::Endpoint::Broken(
                        capnp::ErrorKind::Failed,
                    )),
                    clients::Location::Broken,
                )
                .unwrap();
            driver.runtime.release(client);
        }
        driver.work_cursor = 5;
        let mut remaining = 8;
        macro_rules! poll {
            () => {
                driver
                    .poll_maintenance_budget_with(
                        messages,
                        &mut queue,
                        512,
                        Default::default(),
                        &mut [0; 8],
                        &mut remaining,
                        &mut || (),
                        &mut |_, _, _| 255,
                        &mut |_, _, _| Ok(None),
                    )
                    .unwrap()
            };
        }
        assert!(matches!(poll!(), Progress::Advanced));
        assert_eq!(remaining, 0);
        assert_eq!(driver.work_cursor, 6);
        assert!(driver.runtime.has_retired());
        remaining = 1;
        assert!(matches!(poll!(), Progress::Advanced));
        assert_eq!(remaining, 0);
        assert_eq!(
            driver.work_cursor, 6,
            "the next rotation skips empty phases and charges one retired owner"
        );
        // The first burst freed exactly eight owners. A single subsequent
        // step frees one more; it cannot drain both remaining owners.
        assert!(driver.drain_endpoint().is_some());
        assert!(driver.drain_endpoint().is_none());
        assert!(queue.is_empty());
        close(driver, messages, &mut queue);
    });
}

#[test]
fn productive_maintenance_preserves_unused_budget_for_following_cleanup() {
    with_driver(|mut driver, messages| {
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        let mut remaining = 0;
        macro_rules! poll {
            () => {
                driver
                    .poll_maintenance_budget_with(
                        messages,
                        &mut queue,
                        512,
                        Default::default(),
                        &mut [0; 8],
                        &mut remaining,
                        &mut || (),
                        &mut |_, _, _| 255,
                        &mut |_, _, _| Ok(None),
                    )
                    .unwrap()
            };
        }
        assert!(matches!(poll!(), Progress::Yield));
        assert_eq!(remaining, 0);
        remaining = 1;
        assert!(matches!(poll!(), Progress::Idle));
        assert_eq!(remaining, 0);

        for owner in [91, 92] {
            let client = driver
                .runtime
                .endpoint(binding::Route::local(owner), clients::Location::Local)
                .unwrap();
            driver.runtime.release(client);
        }
        driver.work_cursor = 5;
        remaining = 128;
        let Progress::Server(first) = poll!() else {
            panic!("first retired endpoint")
        };
        assert_eq!(
            remaining, 127,
            "a productive step must not consume the whole allowance"
        );
        let Progress::Server(second) = poll!() else {
            panic!("second retired endpoint")
        };
        assert!(remaining > 0 && remaining < 127);
        let mut owners = [first, second];
        owners.sort_unstable();
        assert_eq!(owners, [91, 92]);
        let before_idle = remaining;
        assert!(matches!(poll!(), Progress::Idle));
        assert_eq!(remaining, before_idle - 1);
        assert!(!driver.runtime.has_retired());
        assert!(queue.is_empty());
        close(driver, messages, &mut queue);
    });
}

#[test]
fn pending_pipeline_callback_exposes_new_cleanup_in_the_same_maintenance_poll() {
    with_driver(|mut driver, messages| {
        let (call, target) = request(&mut driver, messages, false);
        let parent = driver.ledgers.answers.pipeline(call.answer().id()).unwrap();
        let observer = driver
            .pipelines
            .bind(
                &mut driver.runtime,
                &mut driver.ledgers.answers,
                &parent,
                pipeline::Path::new(&[]),
            )
            .unwrap();
        driver.ledgers.answers.release_pipeline_deferred(parent);
        let mut retiring = Some(
            driver
                .runtime
                .endpoint(binding::Route::local(99), clients::Location::Local)
                .unwrap(),
        );
        assert!(!driver.runtime.has_retired());
        driver.work_cursor = 2;
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        let outcome = driver
            .poll_maintenance_with(
                messages,
                &mut queue,
                512,
                Default::default(),
                &mut [0; 8],
                NonZeroUsize::new(32).unwrap(),
                &mut || (),
                &mut |_, _, _| 255,
                &mut |_, _, graph| {
                    graph.release(retiring.take().unwrap());
                    Ok(None)
                },
            )
            .unwrap();
        assert!(retiring.is_none());
        assert!(matches!(outcome, Progress::Server(99)));
        driver.runtime.release(observer);
        driver.release_call(call, messages);
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn maintenance_uses_early_pipeline_before_failed_result() {
    with_driver(|mut driver, messages| {
        let (call, target) = request(&mut driver, messages, false);
        let parent = driver.ledgers.answers.pipeline(call.answer().id()).unwrap();
        let observer = driver
            .pipelines
            .bind(
                &mut driver.runtime,
                &mut driver.ledgers.answers,
                &parent,
                pipeline::Path::new(&[]),
            )
            .unwrap();
        driver.ledgers.answers.release_pipeline_deferred(parent);
        driver
            .complete_answer(&call, Err(capnp::ErrorKind::Failed), messages)
            .unwrap_or_else(|_| panic!());
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        let mut invoked = false;
        for _ in 0..128 {
            driver
                .poll_maintenance_with(
                    messages,
                    &mut queue,
                    512,
                    Default::default(),
                    &mut [0; 8],
                    NonZeroUsize::new(1).unwrap(),
                    &mut || (),
                    &mut |_, _, _| 255,
                    &mut |_, _, graph| {
                        invoked = true;
                        graph
                            .retain(&target)
                            .map(|c| Some(Ok(c)))
                            .map_err(pipelines::Error::Graph)
                    },
                )
                .unwrap();
            if invoked {
                break;
            }
        }
        assert!(invoked);
        assert!(matches!(
            driver.runtime.inspect(&observer).unwrap(),
            clients::Destination::Endpoint {
                location: clients::Location::Local,
                ..
            }
        ));
        driver.runtime.release(observer);
        driver.release_call(call, messages);
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn bootstrap_completion_retries_and_preserves_pipelines_after_finish_without_allocation() {
    for failed in [false, true] {
        for finished in [false, true] {
            let measured = allocation_counter::measure(|| {
                with_driver(|mut driver, messages| {
                    let target = driver
                        .runtime
                        .endpoint(binding::Route::local(91), clients::Location::Local)
                        .unwrap();
                    let frame = Frame::build(messages, 512, Default::default(), |b| {
                        calls::bootstrap(b, QuestionId::from_wire(3))
                    })
                    .unwrap();
                    let Received::Bootstrap(bootstrap) = driver
                        .receive(frame, messages, &mut [0; 8], |_| (), || 73)
                        .unwrap_or_else(|e| panic!("{}", e.error))
                    else {
                        panic!("bootstrap")
                    };
                    let key = bootstrap.answer();
                    let parent = driver.ledgers.answers.pipeline(key.id()).unwrap();
                    let observer = driver
                        .pipelines
                        .bind(
                            &mut driver.runtime,
                            &mut driver.ledgers.answers,
                            &parent,
                            pipeline::Path::new(&[]),
                        )
                        .unwrap();
                    driver.ledgers.answers.release_pipeline_deferred(parent);
                    // Exhausted storage rolls back completion, including its clients.
                    if !failed {
                        let held =
                            core::array::from_fn::<_, 6, _>(|_| messages.reserve(1024).unwrap());
                        assert!(driver
                            .complete_bootstrap(
                                &bootstrap,
                                Ok(&target),
                                messages,
                                512,
                                Default::default()
                            )
                            .is_err());
                        for writer in held {
                            messages.discard(writer);
                        }
                    }
                    driver
                        .complete_bootstrap(
                            &bootstrap,
                            if failed {
                                Err(capnp::ErrorKind::Overloaded)
                            } else {
                                Ok(&target)
                            },
                            messages,
                            512,
                            Default::default(),
                        )
                        .unwrap();
                    assert_eq!(
                        driver.complete_bootstrap(
                            &bootstrap,
                            Err(capnp::ErrorKind::Failed),
                            messages,
                            512,
                            Default::default()
                        ),
                        Err(Error::Pipeline(pipelines::Error::AlreadyCompleted))
                    );
                    let mut slots = [None];
                    let mut queue = messages::Queue::new(&mut slots).unwrap();
                    let blocker = Frame::build(messages, 512, Default::default(), |b| {
                        calls::acknowledge(b, key.id())
                    })
                    .unwrap();
                    let offer = messages.offer(&blocker.lease).unwrap();
                    queue.reserve().unwrap().accept(messages, offer);
                    blocker.release(messages);
                    assert!(driver
                        .send_answer(
                            key,
                            messages,
                            &mut queue,
                            512,
                            Default::default(),
                            &mut [0; 8],
                            |_, _| Ok(())
                        )
                        .is_err());
                    drain(&mut queue, messages, |_| ());
                    if finished {
                        assert!(driver
                            .ledgers
                            .answers
                            .finish(key.id(), false)
                            .retired
                            .is_none());
                    }
                    driver
                        .send_answer(
                            key,
                            messages,
                            &mut queue,
                            512,
                            Default::default(),
                            &mut [0; 8],
                            |producer, mut exception| {
                                assert_eq!(*producer, 73);
                                exception
                                    .field(Exception::REASON)
                                    .copy_from("bootstrap refused")
                            },
                        )
                        .unwrap();
                    drain(&mut queue, messages, |root| {
                        let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                            panic!("return")
                        };
                        let returned = returned.unwrap();
                        assert_eq!(returned.get_answer_id(), 3);
                        match returned.which().unwrap() {
                            rpc_capnp::return_::Canceled(()) => assert!(finished),
                            rpc_capnp::return_::Exception(exception) => {
                                assert!(failed && !finished);
                                assert_eq!(
                                    exception.unwrap().get_reason().unwrap().to_str().unwrap(),
                                    "bootstrap refused"
                                );
                            }
                            rpc_capnp::return_::Results(payload) => {
                                assert!(!failed && !finished);
                                let payload = payload.unwrap();
                                assert_eq!(payload.get_content().capability_index().unwrap(), 0);
                                assert_eq!(payload.get_cap_table().unwrap().len(), 1);
                            }
                            _ => panic!("wrong return"),
                        }
                    });
                    assert_eq!(
                        driver.poll_pipeline(0, messages).unwrap(),
                        Propagation::Resolved
                    );
                    match driver.runtime.inspect(&observer).unwrap() {
                        clients::Destination::Endpoint {
                            owner: binding::Route::Capability(binding::Endpoint::Local(91)),
                            ..
                        } => assert!(!failed),
                        clients::Destination::Endpoint {
                            owner:
                                binding::Route::Capability(binding::Endpoint::Broken(
                                    capnp::ErrorKind::Overloaded,
                                )),
                            ..
                        } => assert!(failed),
                        _ => panic!("pipeline"),
                    }
                    // Finish permits wire-ID reuse but cannot redirect the old token.
                    if finished {
                        let frame = Frame::build(messages, 512, Default::default(), |b| {
                            calls::bootstrap(b, QuestionId::from_wire(3))
                        })
                        .unwrap();
                        let Received::Bootstrap(new) = driver
                            .receive(frame, messages, &mut [0; 8], |_| (), || 74)
                            .unwrap_or_else(|e| panic!("{}", e.error))
                        else {
                            panic!("bootstrap")
                        };
                        assert_ne!(key, new.answer());
                        assert!(driver
                            .complete_bootstrap(
                                &bootstrap,
                                Ok(&target),
                                messages,
                                512,
                                Default::default()
                            )
                            .is_err());
                        driver
                            .complete_bootstrap(
                                &new,
                                Ok(&target),
                                messages,
                                512,
                                Default::default(),
                            )
                            .unwrap();
                    }
                    driver.runtime.release(observer);
                    driver.runtime.release(target);
                    close(driver, messages, &mut queue);
                })
            });
            assert_eq!(measured.count_total, 0);
        }
    }
}

#[test]
fn empty_work_classes_do_not_delay_the_next_owned_operation() {
    for phase in 0..16 {
        with_driver(|mut driver, messages| {
            let client = driver
                .runtime
                .endpoint(binding::Route::local(99), clients::Location::Local)
                .unwrap();
            driver.runtime.release(client);
            driver.work_cursor = phase;
            let mut slots = [None];
            let mut queue = messages::Queue::new(&mut slots).unwrap();
            let mut remaining = 1;
            let outcome = driver
                .poll_maintenance_budget_with(
                    messages,
                    &mut queue,
                    512,
                    Default::default(),
                    &mut [0; 8],
                    &mut remaining,
                    &mut || (),
                    &mut |_, _, _| 255,
                    &mut |_, _, _| Ok(None),
                )
                .unwrap();
            assert!(
                matches!(outcome, Progress::Server(99)),
                "starting phase {phase}"
            );
            assert_eq!(remaining, 0);
            assert_eq!(driver.work_cursor, 6);
            assert!(!driver.runtime.has_retired());
            close(driver, messages, &mut queue);
        });
    }
}

#[test]
fn unsupported_frames_echo_without_admitting_answers_or_losing_retry_ownership() {
    let measured = allocation_counter::measure(|| {
        for tag in [u16::MAX, 7, 9, 12] {
            with_driver(|mut driver, messages| {
                driver.allow_join(false);
                let frame = Frame::build(messages, 256, Default::default(), |builder| {
                    let mut root = builder.try_init_root::<Message>()?;
                    // Unknown, obsolete Save/Delete, and disabled Join. The
                    // opaque pointer must survive even without a target lease.
                    root.field(Message::OBSOLETE_SAVE)
                        .cast::<capnp::field_api::Text>()
                        .copy_from("opaque body")?;
                    root.scalar(capnp::field_api::ScalarDescriptor::new(0, 0u16, None))
                        .set(tag);
                    Ok(())
                })
                .unwrap();
                assert!(matches!(
                    driver
                        .receive_with_policy(frame, messages, &mut [], &mut |_| (), |_| {
                            panic!("unsupported messages have no application producer")
                        })
                        .unwrap_or_else(|r| panic!("{:?}", r.error)),
                    Received::Network
                ));
                assert!(driver.ledgers.answers.is_empty());
                let mut empty = [];
                let mut blocked = messages::Queue::new(&mut empty).unwrap();
                assert!(driver
                    .send_unimplemented(messages, &mut blocked, Default::default())
                    .is_err());
                assert!(matches!(
                    driver.peek_network(),
                    Some(NetworkRequest::Unsupported(_))
                ));
                let mut slots = [None];
                let mut queue = messages::Queue::new(&mut slots).unwrap();
                driver
                    .send_unimplemented(messages, &mut queue, Default::default())
                    .unwrap();
                assert!(!driver.has_network());
                let window = queue.window(messages).unwrap().unwrap();
                let length = window.bytes().len();
                {
                    let mut bytes = window.bytes();
                    let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                        &mut bytes,
                        Default::default(),
                    )
                    .unwrap();
                    let root = reader.get_root::<rpc_capnp::message::Reader>().unwrap();
                    let rpc_capnp::message::Unimplemented(original) = root.which().unwrap() else {
                        panic!("echo")
                    };
                    use capnp::traits::IntoInternalStructReader;
                    assert_eq!(
                        original
                            .unwrap()
                            .into_internal_struct_reader()
                            .get_data_field::<u16>(0),
                        tag
                    );
                }
                window.advance(length).unwrap();
                assert!(driver.ledgers.answers.is_empty());
                close(driver, messages, &mut queue);
            });
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn dispatch_releases_target_once_while_retaining_parameters_and_answer_authority() {
    let measured = allocation_counter::measure(|| {
        with_driver(|mut driver, messages| {
            let (mut call, target) = request(&mut driver, messages, false);
            assert!(call.target().is_some());
            driver.release_call_target(&mut call);
            driver.release_call_target(&mut call);
            assert!(call.target().is_none());
            call.with(messages, |params| {
                assert_eq!(params.incoming().hints().interface, 7)
            })
            .unwrap();
            let result = driver
                .local_result_dynamic(
                    7,
                    9,
                    core::iter::empty(),
                    messages,
                    128,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap();
            driver
                .complete_answer(&call, Ok(result), messages)
                .unwrap_or_else(|_| panic!("live answer"));
            let mut slots = [None];
            let mut queue = messages::Queue::new(&mut slots).unwrap();
            let _ = driver
                .send_answer(
                    call.answer(),
                    messages,
                    &mut queue,
                    128,
                    Default::default(),
                    &mut [0; 8],
                    |_, _| Ok(()),
                )
                .unwrap();
            driver.release_call(call, messages);
            drain(&mut queue, messages, |root| {
                assert!(matches!(
                    root.which().unwrap(),
                    rpc_capnp::message::Return(_)
                ))
            });
            driver.capabilities().release(target);
            close(driver, messages, &mut queue);
        });
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn bilateral_join_results_retry_output_and_keep_explicit_finish_with_or_without_capabilities() {
    let measured = allocation_counter::measure(|| {
        for capability in [false, true] {
            with_driver(|mut driver, messages| {
                let target = driver
                    .capabilities()
                    .endpoint(binding::Route::local(91), clients::Location::Local)
                    .unwrap();
                let id = {
                    let (mut graph, mut batch) = driver.export_batch();
                    let grant = binding::stage(&mut graph, &mut batch, &target).unwrap();
                    batch.commit();
                    grant.id()
                };
                let request = Frame::build(messages, 256, Default::default(), |builder| {
                    crate::network::join(
                        builder,
                        QuestionId::from_wire(7),
                        calls::Target::Imported(ImportId::from_wire(id.to_wire())),
                        |_| Ok(()),
                    )
                })
                .unwrap();
                driver
                    .receive(request, messages, &mut [0; 8], |_| (), || 73)
                    .unwrap_or_else(|r| panic!("{:?}", r.error));
                let NetworkRequest::Join(join) = driver.take_network().unwrap() else {
                    panic!("Join")
                };
                let key = join.key();
                let result = driver
                    .local_result_dynamic(
                        0,
                        0,
                        core::iter::repeat_n(Some(&target), usize::from(capability)),
                        messages,
                        256,
                        Default::default(),
                        |field| {
                            if capability {
                                field.set_capability_index(0)
                            } else {
                                field.cast::<capnp::field_api::Data>().copy_from(b"joined")
                            }
                        },
                    )
                    .unwrap();
                driver
                    .complete_join_results(&join, result, messages)
                    .unwrap_or_else(|_| panic!("completion"));
                let mut empty = [];
                let mut blocked = messages::Queue::new(&mut empty).unwrap();
                let rejected = driver
                    .send_join_results(
                        join,
                        messages,
                        &mut blocked,
                        256,
                        Default::default(),
                        &mut [0; 8],
                    )
                    .err()
                    .unwrap();
                assert_eq!(rejected.join.key(), key);
                let mut slots = [None];
                let mut queue = messages::Queue::new(&mut slots).unwrap();
                assert!(driver
                    .send_join_results(
                        rejected.join,
                        messages,
                        &mut queue,
                        256,
                        Default::default(),
                        &mut [0; 8]
                    )
                    .unwrap_or_else(|r| panic!("{:?}", r.error))
                    .is_none());
                drain(&mut queue, messages, |root| {
                    let rpc_capnp::message::Return(reply) = root.which().unwrap() else {
                        panic!("Return")
                    };
                    let reply = reply.unwrap();
                    assert!(!reply.get_no_finish_needed());
                    let rpc_capnp::return_::Results(payload) = reply.which().unwrap() else {
                        panic!("results")
                    };
                    assert_eq!(
                        payload.unwrap().get_cap_table().unwrap().len(),
                        u32::from(capability)
                    );
                });
                let finish = Frame::build(messages, 128, Default::default(), |builder| {
                    use crate::rpc_capnp::api::Finish;
                    let root = builder.try_init_root::<Message>()?;
                    let mut finish = root.into_field(Message::FINISH).init()?;
                    finish.scalar(Finish::QUESTION_ID).set(7);
                    finish.scalar(Finish::RELEASE_RESULT_CAPS).set(true);
                    Ok(())
                })
                .unwrap();
                assert!(matches!(
                    driver
                        .receive(
                            finish,
                            messages,
                            &mut [0; 8],
                            |_| (),
                            || panic!("Finish has no producer")
                        )
                        .unwrap_or_else(|r| panic!("{:?}", r.error)),
                    Received::Finished(Some(73))
                ));
                let _ = driver.release_export(id, 1).unwrap();
                driver.capabilities().release(target);
                close(driver, messages, &mut queue);
            });
        }
    });
    assert_eq!(measured.count_total, 0);
}
