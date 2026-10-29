//! Hosted introduction policy and generation-owned forwarding vines.
use super::*;
use capntproto_rpc_engine::{
    clients::Location,
    ids::{ExportId, WireId},
};
use capntproto_rpc_wire::rpc_capnp::api::{CapDescriptor, ThirdPartyCapDescriptor};

pub(crate) trait Network {
    fn supports_answers(&self) -> bool;
    fn tail_destination(
        &self,
        contact: any_pointer::Reader<'_>,
    ) -> capnp::Result<super::tail::Destination>;
    fn domain(&self) -> usize;
    fn peer(&self) -> Box<dyn std::any::Any>;
    fn introduce(&self, peer: Box<dyn std::any::Any>) -> capnp::Result<Option<(Body, Body)>>;
    fn accept(
        &self,
        contact: any_pointer::Reader<'_>,
    ) -> capnp::Result<(Vec<u8>, Promise<Box<dyn ClientHook>, Error>)>;
}
pub(crate) type Policy = Rc<dyn Fn(&dyn ClientHook) -> Option<Rc<dyn ClientHook>>>;
pub(super) struct Marker {
    target: Box<dyn ClientHook>,
    id: Vec<u8>,
    _permit: capntproto_rpc_engine::flow_control::outgoing::OwnedPermit,
}
impl Handle {
    pub(crate) fn introduced(
        &self,
        target: Box<dyn ClientHook>,
        contact: any_pointer::Reader<'_>,
    ) -> capnp::Result<Box<dyn ClientHook>> {
        let commands = self.0.upgrade().ok_or_else(closed)?;
        let network = commands.network.borrow().clone().ok_or_else(closed)?;
        let permit = commands
            .capacity
            .reserve_owned()
            .map_err(|_| Error::overloaded("introduction capacity exhausted".into()))?;
        commands
            .markers
            .borrow_mut()
            .try_reserve(1)
            .map_err(wire_error)?;
        let (id, future) = network.accept(contact)?;
        if id.len() > commands.bytes.saturating_sub(128) {
            return Err(Error::overloaded("embargo exceeds frame budget".into()));
        }
        // The marker's FIFO publication is not the remote acceptance fence.
        // Keep the forwarding vine until Accept completes, so its Provide
        // cannot finish while the destination is still dispatching the marker.
        let accepting_vine = target.add_ref();
        commands.markers.borrow_mut().push_back(Marker {
            target,
            id,
            _permit: permit,
        });
        commands.notify();
        let client: capability::Client = crate::new_future_client(async move {
            let result = future.await;
            drop(accepting_vine);
            result.map(capability::Client::new)
        });
        Ok(client.hook)
    }
}

pub(super) struct Provision {
    target: Box<dyn ClientHook>,
    recipient: Body,
    markers: RefCell<VecDeque<Vec<u8>>>,
    commands: Weak<Commands>,
    _permit: capntproto_rpc_engine::flow_control::outgoing::OwnedPermit,
}
impl Drop for Provision {
    fn drop(&mut self) {
        if let Some(commands) = self.commands.upgrade() {
            commands.notify();
        }
    }
}
pub(super) struct Provided<'id> {
    owner: Weak<Provision>,
    pub(super) wire: questions::Provision<'id>,
}
struct Vine {
    inner: Box<dyn ClientHook>,
    contact: Rc<Body>,
    provision: Rc<Provision>,
}
impl Vine {
    fn forward(&self, id: &[u8]) -> capnp::Result<()> {
        let Some(commands) = self.provision.commands.upgrade() else {
            return Ok(());
        };
        // A lost upstream provision rejects its acceptance. It must not tear
        // down this recipient's otherwise healthy connection.
        if commands.closed.borrow().is_some() {
            return Ok(());
        }
        let mut markers = self.provision.markers.borrow_mut();
        if markers.len() >= commands.capacity.limit()
            || id.len() > commands.bytes.saturating_sub(128)
        {
            return Err(Error::overloaded(
                "provision marker capacity exhausted".into(),
            ));
        }
        markers.try_reserve(1).map_err(wire_error)?;
        // Release can follow the received marker immediately, dropping the
        // last exported Vine before its source connection is polled again.
        // Keep one owner per nonempty marker queue until FIFO publication.
        if markers.is_empty() {
            let mut forwarding = commands.forwarding.borrow_mut();
            forwarding.try_reserve(1).map_err(wire_error)?;
            forwarding.push_back(self.provision.clone());
        }
        markers.push_back(id.to_vec());
        drop(markers);
        commands.notify();
        Ok(())
    }
}
impl ClientHook for Vine {
    fn get_failure(&self) -> Option<Error> {
        self.inner.get_failure()
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(Self {
            inner: self.inner.add_ref(),
            contact: self.contact.clone(),
            provision: self.provision.clone(),
        })
    }
    fn new_call(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
    ) -> capability::Request<any_pointer::Owned, any_pointer::Owned> {
        capability::Request::new(Box::new(crate::local::Request::new(
            interface,
            method,
            size,
            self.add_ref(),
        )))
    }
    fn call(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        self.call_with_hints(interface, method, params, results, CallHints::default())
    }
    fn call_with_hints(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
        hints: CallHints,
    ) -> Promise<(), Error> {
        use crate::attach::Attach;
        Promise::from_future(
            self.inner
                .call_with_hints(interface, method, params, results, hints)
                .attach(self.provision.clone()),
        )
    }
    fn get_brand(&self) -> usize {
        0
    }
    fn get_ptr(&self) -> usize {
        Rc::as_ptr(&self.provision) as usize
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        None
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        None
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        Promise::ok(())
    }
    fn forward_join(&self, part: any_pointer::Reader<'_>) -> Option<PartResponse> {
        self.inner.forward_join(part)
    }
    fn metadata_changes(&self) -> Option<Rc<capnp::private::capability::MetadataChanges>> {
        self.inner.metadata_changes()
    }
    #[cfg(unix)]
    fn get_fd(&self) -> Option<Rc<std::os::fd::OwnedFd>> {
        self.inner.get_fd()
    }
}
pub(crate) fn policy(handle: Handle) -> Policy {
    Rc::new(move |hook| {
        let destination = handle.0.upgrade()?;
        let network = destination.network.borrow().clone()?;
        let hook = hook.as_any()?.downcast_ref::<crate::scoped::Hook>()?;
        if !hook.0.settled.get() || !hook.0.remote.get() {
            return None;
        }
        let source = hook.0.inbox.upgrade()?.joins.as_ref()?.0.upgrade()?;
        if Rc::ptr_eq(&source, &destination) {
            return None;
        }
        let host = source.network.borrow().clone()?;
        if host.domain() != network.domain() {
            return None;
        }
        let (contact, recipient) = host.introduce(network.peer()).ok()??;
        for body in [&contact, &recipient] {
            let reader = body.get_root_as_reader::<any_pointer::Reader>().ok()?;
            reader.validate_capability_indices(0).ok()?;
            if minimum(reader).ok()? > source.bytes {
                return None;
            }
        }
        let permit = source.capacity.reserve_owned().ok()?;
        let provision = Rc::new(Provision {
            target: hook.add_ref(),
            recipient,
            markers: RefCell::default(),
            commands: Rc::downgrade(&source),
            _permit: permit,
        });
        {
            let mut queue = source.provisions.borrow_mut();
            queue.try_reserve(1).ok()?;
            queue.push_back(provision.clone());
        }
        source.notify();
        Some(Rc::new(Vine {
            inner: hook.add_ref(),
            contact: Rc::new(contact),
            provision,
        }) as Rc<dyn ClientHook>)
    })
}
pub(crate) fn is_vine(hook: &dyn ClientHook) -> bool {
    hook.as_any().is_some_and(|a| a.is::<Vine>())
}
fn vine(hook: &Rc<dyn ClientHook>) -> Option<&Vine> {
    hook.as_any()?.downcast_ref()
}
pub(crate) fn descriptors<'id, P: Copy, const H: usize>(
) -> capntproto_rpc_wire::publication::descriptors::Descriptors<Route<'id, P, Rc<dyn ClientHook>, H>>
{
    capntproto_rpc_wire::publication::descriptors::Descriptors {
        identity: crate::scoped::wire::foreign::export_identity,
        extra_bytes: |route| match route {
            Route::Vine(owner) => {
                vine(owner).map_or(Ok(0), |v| minimum(v.contact.get_root_as_reader()?))
            }
            _ => Ok(0),
        },
        write: |route, id: ExportId, mut field| {
            if let Route::Vine(owner) = route {
                if let Some(vine) = vine(owner) {
                    let mut third = field.field(CapDescriptor::THIRD_PARTY_HOSTED).init()?;
                    third
                        .scalar(ThirdPartyCapDescriptor::VINE_ID)
                        .set(id.to_wire());
                    return third
                        .field(ThirdPartyCapDescriptor::ID)
                        .copy_from(vine.contact.get_root_as_reader::<any_pointer::Reader>()?);
                }
            }
            field.scalar(CapDescriptor::SENDER_HOSTED).set(id.to_wire());
            Ok(())
        },
    }
}

impl<
        'p,
        'id,
        V: 'static,
        F: FnMut() -> capnp::Result<capability::Client>,
        const C: usize,
        const H: usize,
    > NetworkControl<'p, 'id, V, F, C, H>
{
    pub(super) fn poll_provisions<'s>(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, 's, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
    ) -> capnp::Result<bool> {
        let marker = self.commands.markers.borrow_mut().pop_front();
        if let Some(marker) = marker {
            let target = coordinator
                .hooks
                .try_retain(&mut coordinator.driver.capabilities(), &*marker.target)?
                .ok_or_else(|| Error::failed("foreign vine marker".into()))?;
            let result = coordinator.driver.send_accept_embargo(
                &target,
                &marker.id,
                messages,
                queue,
                marker.id.len().saturating_add(128 + H * 16),
                coordinator.options.reader,
            );
            coordinator.driver.capabilities().release(target);
            result.map_err(wire_error)?;
            return Ok(true);
        }
        for index in (0..self.provided.len()).rev() {
            let Some(owner) = self.provided[index].owner.upgrade() else {
                coordinator
                    .driver
                    .release_provision(self.provided.swap_remove(index).wire);
                return Ok(true);
            };
            let id = owner.markers.borrow().front().cloned();
            if let Some(id) = id {
                coordinator
                    .driver
                    .send_provision_embargo(
                        &self.provided[index].wire,
                        &id,
                        messages,
                        queue,
                        id.len().saturating_add(128),
                        coordinator.options.reader,
                    )
                    .map_err(wire_error)?;
                drop(owner.markers.borrow_mut().pop_front());
                if owner.markers.borrow().is_empty() {
                    let retained = {
                        let mut forwarding = self.commands.forwarding.borrow_mut();
                        forwarding
                            .iter()
                            .position(|provision| Rc::ptr_eq(provision, &owner))
                            .and_then(|index| forwarding.remove(index))
                    };
                    drop(retained);
                }
                return Ok(true);
            }
        }
        let provision = self.commands.provisions.borrow_mut().pop_front();
        let Some(provision) = provision else {
            return Ok(false);
        };
        self.provided.try_reserve(1).map_err(wire_error)?;
        let target = wire_target(&*provision.target, coordinator)?;
        let recipient = provision
            .recipient
            .get_root_as_reader::<any_pointer::Reader>()?;
        let wire = coordinator
            .driver
            .send_provide(
                messages,
                queue,
                minimum(recipient)?.saturating_add(H * 16),
                coordinator.options.reader,
                |builder, question| {
                    capntproto_rpc_wire::network::provide(
                        builder,
                        question,
                        target.borrowed(),
                        |field| field.copy_from(recipient),
                    )
                },
            )
            .map_err(wire_error)?;
        self.provided.push(Provided {
            owner: Rc::downgrade(&provision),
            wire,
        });
        Ok(true)
    }
}
pub(super) fn wire_target<'p, 'id, const C: usize, const H: usize>(
    hook: &dyn ClientHook,
    coordinator: &mut Coordinator<'p, 'id, '_, resources::Import, (), C, H>,
) -> capnp::Result<Target> {
    let driver = &mut coordinator.driver;
    let client = coordinator
        .hooks
        .try_retain(&mut driver.capabilities(), hook)?
        .ok_or_else(|| Error::failed("foreign introduction target".into()))?;
    let result = (|| {
        let Destination::Endpoint {
            owner,
            location: Location::Remote(_),
            ..
        } = driver.runtime().inspect(&client).map_err(wire_error)?
        else {
            return Err(Error::failed("introduction target is not remote".into()));
        };
        match owner
            .target(&driver.ledgers().questions)
            .map_err(wire_error)?
        {
            Some(capntproto_rpc_wire::calls::Target::Imported(id)) => Ok(Target::Imported(id)),
            Some(capntproto_rpc_wire::calls::Target::Promised { question, path }) => {
                Ok(Target::Promised(question, path.fields().to_vec()))
            }
            None => Err(Error::failed("introduction lost its route".into())),
        }
    })();
    driver.capabilities().release(client);
    result
}
pub(super) fn forward(owner: &Rc<dyn ClientHook>, id: &[u8]) -> capnp::Result<()> {
    vine(owner)
        .ok_or_else(|| Error::failed("unregistered forwarding vine".into()))?
        .forward(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_markers_retain_the_provision_after_the_last_vine_is_released() {
        let commands = Commands::new(2, 1024);
        let provision = Rc::new(Provision {
            target: crate::broken::new_cap(closed()),
            recipient: Body::new_default(),
            markers: RefCell::default(),
            commands: Rc::downgrade(&commands),
            _permit: commands.capacity.reserve_owned().unwrap(),
        });
        let observer = Rc::downgrade(&provision);
        let vine = Vine {
            inner: crate::broken::new_cap(closed()),
            contact: Rc::new(Body::new_default()),
            provision,
        };
        vine.forward(b"first").unwrap();
        vine.forward(b"second").unwrap();
        assert!(vine.forward(b"over capacity").is_err());
        drop(vine);
        let retained = observer
            .upgrade()
            .expect("queued embargoes keep Provide alive");
        assert_eq!(retained.markers.borrow().len(), 2);
        assert_eq!(commands.forwarding.borrow().len(), 1);
        drop(retained);
        // Terminal teardown releases these queued owners even without a sender.
        commands.close(closed());
        assert!(observer.upgrade().is_none());
    }
}
