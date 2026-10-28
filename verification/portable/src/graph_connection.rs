//! Imports own remote obligations, graph endpoints own imports, and exports
//! own graph routes, all inside a single caller-backed connection domain.
use capntproto_rpc_engine::{
    answers,
    clients::{self, dispatch, Destination, Location},
    connection, exports,
    ids::{ImportId, WireId},
    imports, messages, questions,
};
use capntproto_rpc_wire::{
    clients::{self as binding, Endpoint},
    owned::{answer, pipelines, Frame, Resolution},
    reception::full::Capability,
    rpc_capnp::api::{CapDescriptor, Message, Resolve},
};

pub fn graph_connection() {
    let mut first = [super::ZERO; 64];
    let mut second = [super::ZERO; 64];
    let mut buffers = [
        messages::Buffer::new(capnp::Word::words_to_bytes_mut(&mut first)),
        messages::Buffer::new(capnp::Word::words_to_bytes_mut(&mut second)),
    ];
    messages::with_messages(&mut buffers, &mut [], |messages| {
        connection::with_domain(|domain| {
            let mut imports_slots = [imports::Slot::EMPTY];
            let mut export_slots = [const { exports::Slot::EMPTY }; 2];
            let mut question_slots = [questions::Slot::EMPTY];
            let mut answer_slots =
                [answers::Slot::<answer::State<'_, pipelines::Results<'_, '_, (), 0>>>::EMPTY];
            let mut client_slots = [const { clients::Slot::<Endpoint<'_, u8, u32>, u8>::EMPTY }; 3];
            let mut connection = domain
                .bind(connection::GraphStorage {
                    notifications: capntproto_rpc_engine::clients::NotificationStorage::empty(),
                    imports: &mut imports_slots,
                    exports: &mut export_slots,
                    questions: &mut question_slots,
                    pipeline_questions: &mut [],
                    answers: &mut answer_slots,
                    clients: &mut client_slots,
                })
                .unwrap();
            let imports = &mut connection.ledgers.imports;
            let exports = &mut connection.ledgers.exports;
            let answers = &mut connection.ledgers.answers;
            let graph = &mut connection.clients;
            let id = ImportId::from_wire(7);
            let mut receipts = [imports::ReceiveSlot::new_promise(id, ())];
            imports.prepare_receive(&mut receipts).unwrap().commit();
            let local = receipts[0].take_local().unwrap();
            let mut import_slots = [binding::ImportSlot::EMPTY];
            let mut map = binding::Imports::new(&mut import_slots, 1).unwrap();
            let remote = map.bind(graph, imports, &local).unwrap();
            imports.release(local);
            let mut batch = exports.batch();
            let grant = batch
                .export_promise(graph.retain(&remote).unwrap())
                .unwrap();
            batch.commit();
            let mut tasks = [dispatch::TaskSlot::EMPTY];
            let mut runtime = dispatch::borrow_runtime(graph, &mut tasks).unwrap();
            let lease = exports.lease(grant.id()).unwrap();
            let reflected = binding::reflected(&mut runtime, exports, &lease).unwrap();
            assert_eq!(reflected.key(), remote.key());
            runtime
                .prepare_call(&reflected, &mut Some(42))
                .unwrap()
                .accept();
            runtime.release(reflected);
            let ready = runtime.next_ready().unwrap().unwrap();
            assert!(matches!(ready.destination(), Destination::Endpoint {
            owner: Endpoint::Imported(route), ..
        } if route.id() == id && route.peer() == 1));
            let running = ready.accept();
            assert_eq!(runtime.complete(running), 42);

            // Remote resolution is reflected back to a local server. Calls already
            // sent through the old route impose a loopback ordering barrier.
            let local = runtime
                .endpoint(Endpoint::Local(43), Location::Local)
                .unwrap();
            let mut batch = exports.batch();
            let reflected_grant = batch.export(local).unwrap();
            batch.commit();
            fn make_resolution<'p>(
                messages: &mut messages::Messages<'p, '_, '_>,
                id: ImportId,
                export: capntproto_rpc_engine::ids::ExportId,
            ) -> Frame<'p> {
                Frame::build(messages, 512, Default::default(), |builder| {
                    let mut root = builder.try_init_root::<Message>()?;
                    let mut resolution = root.field(Message::RESOLVE).init()?;
                    resolution.scalar(Resolve::PROMISE_ID).set(id.to_wire());
                    resolution
                        .field(Resolve::CAP)
                        .init()?
                        .scalar(CapDescriptor::RECEIVER_HOSTED)
                        .set(export.to_wire());
                    Ok(())
                })
                .unwrap()
            }
            let frame = make_resolution(messages, id, reflected_grant.id());
            let resolution = Resolution::admit(frame, messages, imports, exports, answers, |_| {
                panic!("reflected owner")
            })
            .unwrap_or_else(|_| panic!("resolve"));
            assert_eq!(
                map.resolution_target(resolution.target().unwrap()),
                Some(remote.key())
            );
            let duplicate = Resolution::admit(
                make_resolution(messages, id, reflected_grant.id()),
                messages,
                imports,
                exports,
                answers,
                |_| panic!("duplicate owner"),
            )
            .err()
            .unwrap();
            duplicate.frame.release(messages);
            assert!(exports.release(reflected_grant.id(), 1).unwrap().is_none());
            resolution
                .with(messages, |view| {
                    assert!(matches!(
                        view.replacement().unwrap(),
                        Some(Capability::Local(_))
                    ))
                })
                .unwrap();
            let mut pipeline_slots = [pipelines::Slot::<0>::EMPTY];
            let mut pipelines = pipelines::Bindings::new(&mut pipeline_slots).unwrap();
            let barrier = resolution
                .apply(
                    messages,
                    &mut runtime,
                    &mut map,
                    imports,
                    exports,
                    answers,
                    &mut pipelines,
                )
                .unwrap_or_else(|_| panic!("apply reflected Resolve"))
                .unwrap();
            runtime.release(
                exports
                    .take_retired(reflected_grant.id().to_wire() as usize)
                    .unwrap(),
            );
            runtime
                .prepare_call(&remote, &mut Some(44))
                .unwrap()
                .accept();
            runtime.release(remote);
            assert!(exports.release(grant.id(), 1).unwrap().is_none());
            runtime.release(exports.release_local(lease).unwrap());
            assert!(runtime.next_ready().unwrap().is_none());
            assert!(imports.pending_release(0).is_none());
            runtime.open(barrier);
            let ready = runtime.next_ready().unwrap().unwrap();
            assert!(matches!(
                ready.destination(),
                Destination::Endpoint {
                    owner: Endpoint::Local(43),
                    ..
                }
            ));
            let running = ready.accept();
            assert_eq!(runtime.complete(running), 44);
            let mut servers = 0;
            while let Some(endpoint) = runtime.take_retired() {
                if let Some(server) = endpoint.release(imports) {
                    assert_eq!(server, 43);
                    servers += 1;
                }
            }
            assert_eq!(servers, 1);
            let release = imports.pending_release(0).unwrap();
            assert_eq!(release.count(), 1);
            release.commit();
            assert!(imports.is_empty());
            assert!(exports.is_empty());
            assert!(runtime.is_empty());
        });
    })
    .unwrap();
}

/// Admission and graph application can be separated by resource pressure. A
/// retry consumes the same admitted Resolve without receiving its grants again.
pub fn graph_resolution_retry() {
    use capntproto_rpc_wire::rpc_capnp::api::{exception::Type, Exception};
    let mut words = [super::ZERO; 64];
    let mut buffers = [messages::Buffer::new(capnp::Word::words_to_bytes_mut(
        &mut words,
    ))];
    messages::with_messages(&mut buffers, &mut [], |messages| {
        connection::with_domain(|domain| {
            let mut is = [imports::Slot::EMPTY];
            let mut es = [];
            let mut qs = [];
            let mut ans =
                [answers::Slot::<answer::State<'_, pipelines::Results<'_, '_, (), 0>>>::EMPTY];
            let mut nodes = [const { clients::Slot::<Endpoint<'_, u8, u32>, u8>::EMPTY }; 3];
            let mut connection = domain
                .bind(connection::GraphStorage {
                    notifications: capntproto_rpc_engine::clients::NotificationStorage::empty(),
                    imports: &mut is,
                    exports: &mut es,
                    questions: &mut qs,
                    pipeline_questions: &mut [],
                    answers: &mut ans,
                    clients: &mut nodes,
                })
                .unwrap();
            let imports = &mut connection.ledgers.imports;
            let exports = &mut connection.ledgers.exports;
            let answers = &mut connection.ledgers.answers;
            let graph = &mut connection.clients;
            let id = ImportId::from_wire(7);
            let mut receipts = [imports::ReceiveSlot::new_promise(id, ())];
            imports.prepare_receive(&mut receipts).unwrap().commit();
            let local = receipts[0].take_local().unwrap();
            let mut slots = [binding::ImportSlot::EMPTY];
            let mut bindings = binding::Imports::new(&mut slots, 1).unwrap();
            let remote = bindings.bind(graph, imports, &local).unwrap();
            imports.release(local);
            let occupying = graph
                .endpoint(Endpoint::Local(99), Location::Local)
                .unwrap();
            let frame = Frame::build(messages, 512, Default::default(), |builder| {
                let mut root = builder.try_init_root::<Message>()?;
                let mut resolve = root.field(Message::RESOLVE).init()?;
                resolve.scalar(Resolve::PROMISE_ID).set(id.to_wire());
                resolve
                    .field(Resolve::EXCEPTION)
                    .init()?
                    .scalar(Exception::TYPE)
                    .set(Type::Overloaded);
                Ok(())
            })
            .unwrap();
            let resolution = Resolution::admit(frame, messages, imports, exports, answers, |_| {
                panic!("exception owns no descriptor")
            })
            .unwrap_or_else(|_| panic!("admission"));
            let mut pipeline_slots = [pipelines::Slot::<0>::EMPTY];
            let mut pipelines = pipelines::Bindings::new(&mut pipeline_slots).unwrap();
            let rejected = resolution
                .apply(
                    messages,
                    graph,
                    &mut bindings,
                    imports,
                    exports,
                    answers,
                    &mut pipelines,
                )
                .err()
                .unwrap();
            assert_eq!(
                rejected.error,
                capntproto_rpc_wire::owned::ApplyError::Graph(clients::Error::Full)
            );
            graph.release(occupying);
            assert_eq!(graph.take_retired().unwrap().release(imports), Some(99));
            assert!(rejected
                .resolution
                .apply(
                    messages,
                    graph,
                    &mut bindings,
                    imports,
                    exports,
                    answers,
                    &mut pipelines
                )
                .unwrap_or_else(|_| panic!("retry"))
                .is_none());
            let call = graph.prepare_call(&remote).unwrap();
            assert!(matches!(
                call.destination(),
                Destination::Endpoint {
                    location: Location::Broken,
                    owner: Endpoint::Broken(capnp::ErrorKind::Overloaded),
                    ..
                }
            ));
            let target = call.accept();
            graph.release(target);
            graph.release(remote);
            while let Some(endpoint) = graph.take_retired() {
                endpoint.release(imports);
            }
            assert_eq!(imports.pending_release(0).unwrap().count(), 1);
            imports.pending_release(0).unwrap().commit();
            assert!(graph.is_empty());
            assert!(imports.is_empty());
            let writer = messages.reserve(512).unwrap();
            messages.discard(writer);
        });
    })
    .unwrap();
}
