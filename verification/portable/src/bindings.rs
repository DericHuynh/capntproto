//! Admitted wire capabilities become owning graph clients with shared resolution.
use super::*;
use capntproto_rpc_engine::{
    answers,
    clients::{self, Destination},
    exports,
    ids::{ImportId, WireId},
    imports,
};
use capntproto_rpc_wire::{
    clients as binding, inbound,
    reception::full::{Capability, Slots},
    rpc_capnp::{
        self,
        api::{CapDescriptor, Message, Resolve},
    },
};
pub fn bounded_imported_clients() -> Result<()> {
    let mut import_slots = [const { imports::Slot::EMPTY }; 2];
    imports::with_imports(&mut import_slots, |imports| -> Result<()> {
        let original = ImportId::from_wire(7);
        let replacement = ImportId::from_wire(9);
        let mut receipts = [
            imports::ReceiveSlot::new_promise(original, 0u32),
            imports::ReceiveSlot::alias(original),
        ];
        imports.prepare_receive(&mut receipts).unwrap().commit();
        let a = receipts[0].take_local().unwrap();
        let b = receipts[1].take_local().unwrap();
        let mut graph_slots = [const { clients::Slot::EMPTY }; 3];
        clients::with_graph(
            &mut graph_slots,
            |graph: &mut clients::Graph<'_, '_, binding::Endpoint<'_, u8, ()>, u8>| -> Result<()> {
                let mut links = [const { binding::ImportSlot::EMPTY }; 2];
                let mut bindings = binding::Imports::new(&mut links, 1).unwrap();
                let first = bindings.bind(graph, imports, &a).unwrap();
                let alias = bindings.bind(graph, imports, &b).unwrap();
                assert_eq!(first.key(), alias.key());
                let frozen = graph.snapshot(&first).unwrap();
                let call = graph.prepare_call(&alias).unwrap().accept();
                graph.release(call);
                imports.release(a);
                imports.release(b);
                let mut exports = [];
                let mut answers = [];
                exports::with_exports(
                    &mut exports,
                    |exports: &mut exports::Exports<'_, '_, ()>| -> Result<()> {
                        answers::with_answers(
                            &mut answers,
                            |answers: &mut answers::Answers<'_, '_, ()>| -> Result<()> {
                                let mut words = [ZERO; 30];
                                let mut builder = message::FixedBuilder::new(&mut words)?;
                                let root = builder.try_init_root::<Message>()?;
                                let mut resolve = root.into_field(Message::RESOLVE).init()?;
                                resolve.scalar(Resolve::PROMISE_ID).set(original.to_wire());
                                resolve
                                    .into_field(Resolve::CAP)
                                    .init()?
                                    .scalar(CapDescriptor::SENDER_HOSTED)
                                    .set(replacement.to_wire());
                                let reader = message::Reader::new(
                                    builder.get_segments_for_output(),
                                    Default::default(),
                                );
                                let rpc_capnp::message::Resolve(resolve) = reader
                                    .get_root::<rpc_capnp::message::Reader<'_>>()?
                                    .which()
                                    .unwrap()
                                else {
                                    unreachable!()
                                };
                                let mut ir = [imports::ReceiveSlot::new(replacement, 99)];
                                let mut er = [exports::LeaseSlot::none()];
                                let mut ar = [answers::PipelineSlot::none()];
                                let resolved = inbound::full::resolve(
                                    resolve?,
                                    imports,
                                    exports,
                                    answers,
                                    Slots {
                                        imports: &mut ir,
                                        exports: &mut er,
                                        answers: &mut ar,
                                    },
                                )
                                .unwrap()
                                .accept();
                                let target = bindings
                                    .resolution_target(resolved.target().unwrap())
                                    .unwrap();
                                let capability = resolved.replacement().unwrap().unwrap();
                                let source = capability.resolution_source();
                                let Capability::Remote(remote) = capability else {
                                    unreachable!()
                                };
                                let mut replacement =
                                    Some(bindings.bind(graph, imports, remote).unwrap());
                                assert!(graph
                                    .prepare_resolution(target, &mut replacement, source)
                                    .unwrap()
                                    .accept()
                                    .is_none());
                                resolved.release(imports, exports, answers);
                                Ok(())
                            },
                        )
                        .unwrap()
                    },
                )
                .unwrap()?;
                for (client, id) in [
                    (&first, replacement),
                    (&alias, replacement),
                    (&frozen, original),
                ] {
                    let call = graph.prepare_call(client).unwrap();
                    let Destination::Endpoint {
                        owner: binding::Endpoint::Imported(remote),
                        ..
                    } = call.destination()
                    else {
                        unreachable!()
                    };
                    assert_eq!(remote.id(), id);
                    assert_eq!(remote.peer(), 1);
                    assert_eq!(imports.wire_id(remote.reference()), id);
                    let task = call.accept();
                    graph.release(task);
                }
                graph.release(first);
                graph.release(alias);
                graph.release(frozen);
                while let Some(owner) = graph.take_retired() {
                    owner.release(imports);
                }
                assert!(graph.is_empty());
                assert_eq!(imports.pending_release(0).unwrap().count(), 2);
                assert_eq!(imports.pending_release(1).unwrap().count(), 1);
                imports.pending_release(0).unwrap().commit();
                imports.pending_release(1).unwrap().commit();
                assert!(imports.is_empty());
                Ok(())
            },
        )
        .unwrap()
    })
    .unwrap()
}
