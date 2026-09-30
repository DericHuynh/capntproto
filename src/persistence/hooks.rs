//! Add the standard Persistent interface to one explicitly bound facet.
//! Requests for every other interface retain their original hooks and hints.
use super::*;
use capnp::{
    any_pointer,
    capability::{CallHints, Request},
    private::capability::{ClientHook, ParamsHook, ResultsHook},
    traits::HasTypeId,
};

type Authorize = Rc<dyn Fn(OwnerId) -> capnp::Result<Option<u64>>>;
struct Save {
    realm: Weak<State>,
    descriptor: Descriptor,
    authorize: Authorize,
    application: Client,
}
impl capnp_rpc::persistent_capnp::persistent::Server<wire::sturdy_ref::Owned, wire::owner::Owned>
    for Save
{
    async fn save(
        self: Rc<Self>,
        params: capnp_rpc::persistent_capnp::persistent::SaveParams<
            wire::sturdy_ref::Owned,
            wire::owner::Owned,
        >,
        mut results: capnp_rpc::persistent_capnp::persistent::SaveResults<
            wire::sturdy_ref::Owned,
            wire::owner::Owned,
        >,
    ) -> capnp::Result<()> {
        // Saving may depend on policy changed by an earlier application stream.
        // Keep the protected save task behind that stream's local barrier.
        self.application.hook.when_local_server_ready().await?;
        let owner = params
            .get()?
            .get_seal_for()?
            .get_id()?
            .try_into()
            .map_err(|_| fail("invalid sealed owner"))?;
        let expires_at = (self.authorize)(owner)?;
        let realm = Realm(self.realm.upgrade().ok_or_else(unavailable)?);
        let reference = realm.issue(owner, &self.descriptor, expires_at)?;
        reference.write(results.get().init_sturdy_ref());
        Ok(())
    }
}
struct Binding {
    application: Client,
    save: Persistent,
}
#[derive(Clone)]
struct Bound(Rc<Binding>);
pub(super) fn bind<C: FromClientHook>(
    client: C,
    realm: Weak<State>,
    descriptor: Descriptor,
    authorize: Authorize,
) -> C {
    let executor = realm.upgrade().and_then(|state| state.executor.clone());
    let application = Client::new(client.into_client_hook());
    let server = Save {
        realm,
        descriptor,
        authorize,
        application: application.clone(),
    };
    let save = match executor {
        Some(executor) => capnp_rpc::new_client_with_executor(server, executor),
        None => capnp_rpc::new_client(server),
    };
    C::new(Box::new(Bound(Rc::new(Binding { application, save }))))
}
impl Bound {
    fn target(&self, interface: u64) -> &dyn ClientHook {
        if interface == Persistent::TYPE_ID {
            self.0.save.as_client_hook()
        } else {
            self.0.application.as_client_hook()
        }
    }
}
impl ClientHook for Bound {
    fn debug_info(&self, chain: &mut capnp::private::capability::DebugInfo) {
        chain.push("persistent");
        chain.follow(self.0.application.as_client_hook());
    }

    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(self.clone())
    }
    fn new_call(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
    ) -> Request<any_pointer::Owned, any_pointer::Owned> {
        self.target(interface).new_call(interface, method, size)
    }
    fn new_call_with_hints(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
        hints: CallHints,
    ) -> Request<any_pointer::Owned, any_pointer::Owned> {
        self.target(interface)
            .new_call_with_hints(interface, method, size, hints)
    }
    fn call(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        self.target(interface)
            .call(interface, method, params, results)
    }
    fn call_with_hints(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
        hints: CallHints,
    ) -> Promise<(), Error> {
        self.target(interface)
            .call_with_hints(interface, method, params, results, hints)
    }
    fn get_brand(&self) -> usize {
        0
    }
    fn get_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    // This is a settled composite facet. Replacing it with the application's
    // resolution would silently discard its Persistent interface.
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        None
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        None
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        Promise::ok(())
    }
    fn is_local_server_ready(&self) -> bool {
        self.0.application.hook.is_local_server_ready()
    }
    fn when_local_server_ready(&self) -> Promise<(), Error> {
        self.0.application.hook.when_local_server_ready()
    }
    #[cfg(unix)]
    fn get_fd(&self) -> Option<Rc<std::os::fd::OwnedFd>> {
        self.0.application.hook.get_fd()
    }
}
