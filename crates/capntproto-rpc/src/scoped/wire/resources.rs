//! Hosted metadata for portable imports. OS resource types stop at this layer;
//! the wire driver accepts the same caller-backed resource receiver on no_std.
use super::*;
use capntproto_rpc_wire::clients::{Endpoint, Route};

/// Application import metadata with optional Unix descriptor ownership.
/// The ledger, retained routes, and owned hooks independently preserve lifetimes.
#[derive(Default)]
pub struct Import<T = ()> {
    pub value: T,
    // Allocate shared state on first metadata or subscription. A subscription
    // preserves future changes even when no descriptor has arrived yet.
    metadata: std::cell::OnceCell<Rc<MetadataSource>>,
}
impl<T> Import<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            metadata: Default::default(),
        }
    }
    /// Keep the first descriptor, including one received through a later alias.
    /// A redundant descriptor is dropped without replacing existing ownership.
    #[cfg(unix)]
    pub fn attach(&mut self, fd: Rc<std::os::fd::OwnedFd>) {
        self.update_metadata(|metadata| {
            if metadata.fd.is_none() {
                metadata.fd = Some(fd);
            }
        });
    }
    pub fn metadata(&self) -> EndpointMetadata {
        self.metadata_if_present().unwrap_or_default()
    }
    pub(crate) fn metadata_if_present(&self) -> Option<EndpointMetadata> {
        self.metadata.get().and_then(|source| source.snapshot())
    }
    pub fn metadata_source(&self) -> Rc<MetadataSource> {
        self.metadata
            .get_or_init(|| Rc::new(MetadataSource::default()))
            .clone()
    }
    pub(crate) fn update_metadata(&mut self, change: impl FnOnce(&mut EndpointMetadata)) {
        self.metadata_source().update(change);
    }
    pub(crate) fn has_introduction(&self) -> bool {
        self.metadata_if_present()
            .as_ref()
            .is_some_and(|metadata| metadata.introduction.is_some() || metadata.failure.is_some())
    }
}

impl<'id> ClientHooks<'id> {
    /// Subscribe imported routes to change sources before publishing
    /// hooks. None declares no remote metadata. Local opaque hooks retain their
    /// policy-preserving observer, since their authority cannot be unwrapped.
    #[inline(always)]
    pub fn collect_wire<I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'_, 'id, '_, I, A, P, S, CAPS, PATH>,
        subscribe: impl FnMut(&I) -> Option<Rc<MetadataSource>>,
        local: impl FnMut(&S) -> EndpointMetadata,
        local_changes: impl FnMut(&S) -> Option<Rc<MetadataChanges>>,
    ) {
        if !self.ownership_ready(&driver.capabilities()) {
            return;
        }
        self.synchronize_wire(driver, subscribe, local, local_changes);
    }
    #[inline(never)]
    fn synchronize_wire<I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'_, 'id, '_, I, A, P, S, CAPS, PATH>,
        mut subscribe: impl FnMut(&I) -> Option<Rc<MetadataSource>>,
        mut local: impl FnMut(&S) -> EndpointMetadata,
        mut local_changes: impl FnMut(&S) -> Option<Rc<MetadataChanges>>,
    ) {
        let (mut graph, ledgers) = driver.split_graph();
        let imports = &ledgers.imports;
        self.synchronize(
            &mut graph,
            |route| match route {
                Route::Capability(Endpoint::Imported(_)) => None,
                Route::Capability(Endpoint::Local(server)) | Route::Vine(server) => {
                    Some(local(server))
                }
                Route::Capability(Endpoint::Failed { kind, owner }) => {
                    let mut metadata = local(owner);
                    metadata
                        .failure
                        .get_or_insert_with(|| Error::from_kind(*kind));
                    Some(metadata)
                }
                Route::Capability(Endpoint::Broken(kind)) => Some(EndpointMetadata {
                    failure: Some(Error::from_kind(*kind)),
                    ..EndpointMetadata::default()
                }),
                Route::Question(_) => None,
            },
            |route| match route {
                Route::Capability(Endpoint::Imported(remote)) => {
                    subscribe(imports.get(remote.reference()))
                }
                _ => None,
            },
            |route| match route {
                Route::Capability(Endpoint::Local(server))
                | Route::Vine(server)
                | Route::Capability(Endpoint::Failed { owner: server, .. }) => {
                    local_changes(server)
                }
                _ => None,
            },
            true,
            true,
        );
    }
}

#[cfg(all(test, unix, not(miri)))]
mod tests {
    use super::*;
    use capntproto_rpc_engine::{
        connection as storage,
        ids::{ImportId, WireId},
        imports,
    };
    use capntproto_rpc_wire::{
        attachments::Receiver,
        owned::Frame,
        rpc_capnp::api::{CapDescriptor, Message, Resolve},
    };
    use std::{
        io::Read,
        num::NonZeroUsize,
        os::{fd::OwnedFd, unix::net::UnixStream},
    };

    #[test]
    fn attached_import_survives_alias_resolution_hook_and_connection_teardown() {
        let (resource, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let fd = Rc::new(OwnedFd::from(resource));
        let weak = Rc::downgrade(&fd);
        let mut attachments = [Some(fd)];
        let mut buffers = messages::GrowingBuffers::new(8, 1024, 8 * 1031);
        let retained = messages::with_growing_messages(
            &mut buffers,
            storage::Growth {
                initial: 0,
                maximum: 8,
            },
            |messages| {
                storage::with_domain(|domain| {
                    let bound = storage::Growth {
                        initial: 0,
                        maximum: 16,
                    };
                    let graph = domain
                        .bind_growing_adoptions(
                            storage::GrowingStorage {
                                notifications:
                                    capntproto_rpc_engine::clients::NotificationGrowth::for_clients(
                                        bound,
                                    ),
                                imports: bound,
                                exports: bound,
                                answers: bound,
                                questions: bound,
                                clients: bound,
                            },
                            bound,
                        )
                        .unwrap();
                    let mut driver: Driver<'_, '_, '_, Import, (), u8, (), 4, 4> =
                        Driver::new_growing(graph, connection::GrowingStorage::uniform(0, 16), 1)
                            .unwrap_or_else(|_| panic!("driver"));
                    let mut receipts = [imports::ReceiveSlot::new_promise(
                        ImportId::from_wire(7),
                        Import::default(),
                    )];
                    driver.prepare_imports(&mut receipts).unwrap().commit();
                    let local = receipts[0].take_local().unwrap();
                    let route = driver.bind_import(&local).unwrap();
                    driver.release_import(local);
                    let mut hooks = ClientHooks::new(4, 4);
                    let client = hooks
                        .export_client(&mut driver.capabilities(), &route)
                        .unwrap();
                    hooks.collect_wire(
                        &mut driver,
                        |owner| Some(owner.metadata_source()),
                        |_| EndpointMetadata::default(),
                        |_| None,
                    );
                    assert!(client.hook.get_fd().is_none());
                    // A late resolution of an abandoned promise gives an existing
                    // import its first resource, without constructing another owner.
                    let frame = Frame::build(messages, 512, Default::default(), |builder| {
                        let mut root = builder.try_init_root::<Message>()?;
                        let mut resolution = root.field(Message::RESOLVE).init()?;
                        resolution.scalar(Resolve::PROMISE_ID).set(123);
                        let mut cap = resolution.field(Resolve::CAP).init()?;
                        cap.scalar(CapDescriptor::SENDER_PROMISE).set(7);
                        cap.scalar(CapDescriptor::ATTACHED_FD).set(0);
                        Ok(())
                    })
                    .unwrap();
                    driver
                        .receive_with_receiver(
                            frame,
                            messages,
                            &mut [0; 16],
                            &mut Receiver::new(
                                &mut attachments,
                                |_| panic!("alias"),
                                Import::attach,
                            ),
                            || (),
                        )
                        .unwrap_or_else(|e| panic!("{}", e.error));
                    assert!(attachments[0].is_none());
                    hooks.collect_wire(
                        &mut driver,
                        |owner| Some(owner.metadata_source()),
                        |_| EndpointMetadata::default(),
                        |_| None,
                    );
                    let saved = client.hook.get_fd().expect("late alias resource");
                    assert!(Rc::ptr_eq(&saved, &weak.upgrade().unwrap()));
                    assert!(!driver.runtime().is_settled(&route).unwrap());
                    // The promise then settles to another capability, carrying a
                    // redundant descriptor. The owned client keeps its first one.
                    let (other, mut other_peer) = UnixStream::pair().unwrap();
                    other_peer
                        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                        .unwrap();
                    let mut next = [Some(Rc::new(OwnedFd::from(other)))];
                    let frame = Frame::build(messages, 512, Default::default(), |builder| {
                        let mut root = builder.try_init_root::<Message>()?;
                        let mut resolution = root.field(Message::RESOLVE).init()?;
                        resolution.scalar(Resolve::PROMISE_ID).set(7);
                        let mut cap = resolution.field(Resolve::CAP).init()?;
                        cap.scalar(CapDescriptor::SENDER_HOSTED).set(9);
                        cap.scalar(CapDescriptor::ATTACHED_FD).set(0);
                        Ok(())
                    })
                    .unwrap();
                    driver
                        .receive_with_receiver(
                            frame,
                            messages,
                            &mut [0; 16],
                            &mut Receiver::new(&mut next, |_| Import::default(), Import::attach),
                            || (),
                        )
                        .unwrap_or_else(|e| panic!("{}", e.error));
                    hooks.collect_wire(
                        &mut driver,
                        |owner| Some(owner.metadata_source()),
                        |_| EndpointMetadata::default(),
                        |_| None,
                    );
                    assert!(driver.runtime().is_settled(&route).unwrap());
                    assert!(Rc::ptr_eq(&saved, &client.hook.get_fd().unwrap()));
                    drop(client);
                    hooks.close(
                        &mut driver.capabilities(),
                        Error::disconnected("test shutdown".into()),
                    );
                    driver.capabilities().release(route);
                    let mut queue = messages::Queue::new_growing(0, 8).unwrap();
                    let mut closing = driver.terminate(&mut queue);
                    let mut done = false;
                    for _ in 0..256 {
                        if matches!(
                            closing
                                .poll(messages, NonZeroUsize::new(16).unwrap())
                                .unwrap(),
                            connection::Termination::Done
                        ) {
                            done = true;
                            break;
                        }
                    }
                    assert!(done);
                    drop(closing);
                    assert_eq!(
                        other_peer.read(&mut [0]).unwrap(),
                        0,
                        "redundant resource closed on teardown"
                    );
                    saved
                })
            },
        )
        .unwrap();
        assert_eq!(
            weak.strong_count(),
            1,
            "only the user's explicit resource retain remains"
        );
        drop(retained);
        assert!(weak.upgrade().is_none());
        assert_eq!(
            peer.read(&mut [0]).unwrap(),
            0,
            "original resource closed exactly after last retain"
        );
    }
}
