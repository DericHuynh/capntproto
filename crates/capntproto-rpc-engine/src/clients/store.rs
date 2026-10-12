//! Capability admission shared by bare graphs and ordered runtimes. The facade
//! deliberately exposes no direct-call admission, so wire bindings can use a
//! Runtime without bypassing its task queue.
use super::*;
pub(super) mod sealed {
    pub trait Sealed {}
}
pub trait Capabilities<'id>: sealed::Sealed {
    type Owner;
    type Peer: Copy + Eq;
    fn is_settled(&self, client: &Client<'id>) -> Result<bool, Error>;
    fn inspect(
        &self,
        client: &Client<'id>,
    ) -> Result<Destination<'id, '_, Self::Owner, Self::Peer>, Error>;
    fn endpoint_with(
        &mut self,
        location: Location<Self::Peer>,
        owner: impl FnOnce() -> Self::Owner,
    ) -> Result<Client<'id>, Error>;
    fn snapshot(&mut self, client: &Client<'id>) -> Result<Client<'id>, Error>;
    fn snapshot_for_export(&mut self, client: &Client<'id>) -> Result<(Client<'id>, bool), Error>;
    fn release(&mut self, client: Client<'id>);
    fn promise(&mut self, initial: &mut Option<Client<'id>>) -> Result<Client<'id>, Error>;
    fn resolve(
        &mut self,
        key: Key<'id>,
        replacement: &mut Option<Client<'id>>,
        source: Source,
    ) -> Result<Option<Barrier<'id>>, Error>;
    fn retain_key(&mut self, key: Key<'id>) -> Result<Client<'id>, Error>;
    fn endpoint(
        &mut self,
        owner: Self::Owner,
        location: Location<Self::Peer>,
    ) -> Result<Client<'id>, Rejected<Self::Owner>>;
    fn imported_promise(
        &mut self,
        owner: Self::Owner,
        peer: Self::Peer,
    ) -> Result<Client<'id>, Rejected<Self::Owner>>;
}
impl<T, P> sealed::Sealed for Graph<'_, '_, T, P> {}
impl<'id, T, P: Copy + Eq> Capabilities<'id> for Graph<'id, '_, T, P> {
    type Owner = T;
    type Peer = P;
    fn is_settled(&self, client: &Client<'id>) -> Result<bool, Error> {
        self.is_settled(client)
    }
    fn inspect(&self, client: &Client<'id>) -> Result<Destination<'id, '_, T, P>, Error> {
        self.inspect(client)
    }
    fn endpoint_with(
        &mut self,
        location: Location<P>,
        owner: impl FnOnce() -> T,
    ) -> Result<Client<'id>, Error> {
        self.endpoint_with(location, owner)
    }
    fn snapshot(&mut self, client: &Client<'id>) -> Result<Client<'id>, Error> {
        self.snapshot(client)
    }
    fn snapshot_for_export(&mut self, client: &Client<'id>) -> Result<(Client<'id>, bool), Error> {
        self.snapshot_for_export(client)
    }
    fn release(&mut self, client: Client<'id>) {
        self.release(client)
    }
    fn promise(&mut self, initial: &mut Option<Client<'id>>) -> Result<Client<'id>, Error> {
        self.promise(initial)
    }
    fn resolve(
        &mut self,
        key: Key<'id>,
        replacement: &mut Option<Client<'id>>,
        source: Source,
    ) -> Result<Option<Barrier<'id>>, Error> {
        Ok(self.prepare_resolution(key, replacement, source)?.accept())
    }
    fn retain_key(&mut self, key: Key<'id>) -> Result<Client<'id>, Error> {
        self.retain_key(key)
    }
    fn endpoint(&mut self, owner: T, location: Location<P>) -> Result<Client<'id>, Rejected<T>> {
        self.endpoint(owner, location)
    }
    fn imported_promise(&mut self, owner: T, peer: P) -> Result<Client<'id>, Rejected<T>> {
        self.imported_promise(owner, peer)
    }
}
