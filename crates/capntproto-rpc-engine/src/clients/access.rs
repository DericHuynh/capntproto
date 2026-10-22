//! A borrowed capability scope exposes transitions without exposing the
//! owning dispatcher for replacement or its raw graph for unordered mutation.
use super::*;
pub struct Access<'a, G> {
    inner: &'a mut G,
}
impl<'a, G> Access<'a, G> {
    pub fn new(inner: &'a mut G) -> Self {
        Self { inner }
    }
}
impl<G> super::store::sealed::Sealed for Access<'_, G> {}
impl<'id, G: Capabilities<'id>> Capabilities<'id> for Access<'_, G> {
    type Owner = G::Owner;
    type Peer = G::Peer;
    fn resources(&self) -> Resources {
        self.inner.resources()
    }
    fn listen(&mut self) -> Result<Listener<'id>, Error> {
        self.inner.listen()
    }
    fn subscribe(
        &mut self,
        listener: &Listener<'id>,
        client: Key<'id>,
        owner: usize,
        watch: Watch,
    ) -> Result<Subscription<'id>, Error> {
        self.inner.subscribe(listener, client, owner, watch)
    }
    fn exclusive_subscription_owner(
        &self,
        listener: &Listener<'id>,
        client: Key<'id>,
    ) -> Option<usize> {
        self.inner.exclusive_subscription_owner(listener, client)
    }
    fn has_events(&self, listener: &Listener<'id>) -> bool {
        self.inner.has_events(listener)
    }
    fn next_event(&mut self, listener: &Listener<'id>) -> Option<Event<'id>> {
        self.inner.next_event(listener)
    }
    fn unsubscribe(&mut self, subscription: Subscription<'id>) {
        self.inner.unsubscribe(subscription)
    }
    fn rebind_subscription(
        &mut self,
        subscription: &Subscription<'id>,
        client: Key<'id>,
    ) -> Result<(), Error> {
        self.inner.rebind_subscription(subscription, client)
    }
    fn close_listener(&mut self, listener: Listener<'id>) {
        self.inner.close_listener(listener)
    }
    fn pending_resolution(&self, client: &Client<'id>) -> Result<Option<Key<'id>>, Error> {
        self.inner.pending_resolution(client)
    }
    fn is_settled(&self, client: &Client<'id>) -> Result<bool, Error> {
        self.inner.is_settled(client)
    }
    fn inspect(
        &self,
        client: &Client<'id>,
    ) -> Result<Destination<'id, '_, G::Owner, G::Peer>, Error> {
        self.inner.inspect(client)
    }
    fn endpoint_with(
        &mut self,
        location: Location<G::Peer>,
        owner: impl FnOnce() -> G::Owner,
    ) -> Result<Client<'id>, Error> {
        self.inner.endpoint_with(location, owner)
    }
    fn snapshot(&mut self, client: &Client<'id>) -> Result<Client<'id>, Error> {
        self.inner.snapshot(client)
    }
    fn snapshot_for_export(&mut self, client: &Client<'id>) -> Result<(Client<'id>, bool), Error> {
        self.inner.snapshot_for_export(client)
    }
    fn release(&mut self, client: Client<'id>) {
        self.inner.release(client)
    }
    fn promise(&mut self, initial: &mut Option<Client<'id>>) -> Result<Client<'id>, Error> {
        self.inner.promise(initial)
    }
    fn resolve(
        &mut self,
        key: Key<'id>,
        replacement: &mut Option<Client<'id>>,
        source: Source,
    ) -> Result<Option<Barrier<'id>>, Error> {
        self.inner.resolve(key, replacement, source)
    }
    fn retain_key(&mut self, key: Key<'id>) -> Result<Client<'id>, Error> {
        self.inner.retain_key(key)
    }
    fn endpoint(
        &mut self,
        owner: G::Owner,
        location: Location<G::Peer>,
    ) -> Result<Client<'id>, Rejected<G::Owner>> {
        self.inner.endpoint(owner, location)
    }
    fn imported_promise(
        &mut self,
        owner: G::Owner,
        peer: G::Peer,
    ) -> Result<Client<'id>, Rejected<G::Owner>> {
        self.inner.imported_promise(owner, peer)
    }
}

impl<'id, 's, 'tasks, T: 's, P: Copy + Eq + 's, C, G: dispatch::GraphStorage<'id, 's, T, P>>
    Access<'_, dispatch::Runtime<'id, 's, 'tasks, T, P, C, G>>
{
    pub fn retain(&mut self, client: &Client<'id>) -> Result<Client<'id>, Error> {
        self.inner.retain_key(client.key())
    }
    pub fn ready(
        &mut self,
    ) -> Result<Option<dispatch::Ready<'_, 'id, 's, T, P, C>>, dispatch::DispatchError> {
        self.inner.ready()
    }
    pub fn next_ready(
        &mut self,
    ) -> Result<Option<dispatch::Ready<'_, 'id, 's, T, P, C>>, dispatch::DispatchError> {
        self.inner.next_ready()
    }
    pub fn advance_dispatch_one(&mut self) -> Result<bool, dispatch::DispatchError> {
        self.inner.advance_dispatch_one()
    }
    pub fn complete<U>(&mut self, task: dispatch::Running<'id, U>) -> U {
        self.inner.complete(task)
    }
}
