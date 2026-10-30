//! Caller-polled resolution of opaque owned hooks imported into the wire graph.
//! Only public hook resolution is observed. Calls keep their original hook even
//! after settlement, so resolving a promise never bypasses policy or overtakes
//! calls that were already admitted through its original proxy.
use super::*;
use capntproto_rpc_engine::clients::{self, Destination, Location};
use capntproto_rpc_wire::clients::{Endpoint, Route};
use std::num::NonZeroUsize;

struct Entry<'id> {
    // A weak, generational graph identity, not a retained capability. Abandoned
    // registrations cannot keep their promise or its protocol exports alive.
    key: clients::Key<'id>,
    subscription: clients::Subscription<'id>,
    state: State,
    wake: Waker,
}
enum State {
    Waiting {
        _hook: Rc<dyn ClientHook>,
        future: Promise<Box<dyn ClientHook>, Error>,
    },
    Ready(capnp::Result<Rc<dyn ClientHook>>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// Exact ready work remains after the supplied budget; poll again.
    Yield,
    Idle,
    Advanced,
    Settled,
    Abandoned,
}

/// Bounded hosted observers, independent of wire import/export/question IDs.
/// Pass `import` to [`Calls::poll`] and [`server::Worker::poll`]. Poll these
/// observers after graph reclamation and before driving calls/maintenance, with
/// the same caller waker. A capacity error retains ready resolution for retry;
/// the caller retries after releasing graph space. `terminate` drops observers
/// before terminal driver teardown; it is not a live-connection cancellation.
#[must_use = "poll imported promises and terminate before driver teardown"]
pub struct ForeignPromises<'id> {
    entries: Vec<Option<Entry<'id>>>,
    listener: Option<clients::Listener<'id>>,
    maximum: usize,
    ready: ready::Queue,
    live: usize,
    first_free: usize,
    prefer_ready: bool,
    introductions: Option<coordinator::multiparty::introduction::Policy>,
}
impl<'id> ForeignPromises<'id> {
    pub fn new(maximum: usize) -> Self {
        Self {
            entries: Vec::new(),
            listener: None,
            maximum,
            ready: ready::Queue::default(),
            live: 0,
            first_free: 0,
            prefer_ready: true,
            introductions: None,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }
    pub(crate) fn with_introductions(
        mut self,
        policy: coordinator::multiparty::introduction::Policy,
    ) -> Self {
        self.introductions = Some(policy);
        self
    }
    /// Import the hook as an opaque local authority. Its public brand/pointer
    /// never grants access to another bridge or permission to unwrap a membrane.
    /// Same-bridge recognition belongs to `ClientHooks::try_retain` first.
    pub fn import<'p, P: Copy + Eq, const H: usize, G>(
        &mut self,
        graph: &mut G,
        hook: &dyn ClientHook,
    ) -> capnp::Result<Client<'id>>
    where
        G: Capabilities<'id, Peer = P, Owner = Route<'p, P, Rc<dyn ClientHook>, H>>,
    {
        // Like legacy exports_by_cap, consult existing ownership before creating
        // another graph endpoint, hook owner or promise-resolution observer.
        // The original source keeps its dispatcher ordering; an independent
        // exported promise's forwarding route would bypass that source.
        if let Some(client) = retain_published(graph, hook)? {
            return Ok(client);
        }
        let mut original = hook.add_ref();
        while let Some(resolved) = original.get_resolved() {
            original = resolved;
        }
        let original: Rc<dyn ClientHook> = Rc::from(original);
        let Some(future) = original.when_more_resolved() else {
            let original = self
                .introductions
                .as_ref()
                .and_then(|policy| policy(&*original))
                .unwrap_or(original);
            let route = if coordinator::multiparty::introduction::is_vine(&*original) {
                Route::Vine(original)
            } else {
                Route::local(original)
            };
            return graph
                .endpoint(route, Location::Local)
                .map_err(|e| graph_error(e.error));
        };
        let index = self.first_free;
        if index == self.entries.len() {
            if index == self.maximum {
                return Err(Error::overloaded(
                    "foreign promise capacity exhausted".into(),
                ));
            }
            self.entries
                .try_reserve(1)
                .map_err(|_| Error::overloaded("foreign promise allocation failed".into()))?;
        }
        if self.listener.is_none() {
            self.listener = Some(graph.listen().map_err(graph_error)?);
        }
        let mut route = Some(
            graph
                .endpoint(Route::local(original.clone()), Location::Local)
                .map_err(|e| graph_error(e.error))?,
        );
        let client = match graph.promise(&mut route) {
            Ok(client) => client,
            Err(error) => {
                graph.release(route.expect("rejected promise retains initial route"));
                return Err(graph_error(error));
            }
        };
        let subscription = match graph.subscribe(
            self.listener.as_ref().unwrap(),
            client.key(),
            index,
            clients::Watch::Abandonment,
        ) {
            Ok(subscription) => subscription,
            Err(error) => {
                graph.release(client);
                return Err(graph_error(error));
            }
        };
        let entry = Some(Entry {
            wake: self.ready.insert(index, true),
            key: client.key(),
            subscription,
            state: State::Waiting {
                _hook: original,
                future,
            },
        });
        if index == self.entries.len() {
            self.entries.push(entry);
        } else {
            self.entries[index] = entry;
        }
        self.live += 1;
        self.first_free = self.entries[index + 1..]
            .iter()
            .position(Option::is_none)
            .map_or(self.entries.len(), |offset| index + 1 + offset);
        self.ready.notify_parent();
        Ok(client)
    }

    pub fn poll<'p, P: Copy + Eq, const H: usize, G>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        graph: &mut G,
        cx: &mut Context<'_>,
        budget: NonZeroUsize,
    ) -> Poll<capnp::Result<Progress>>
    where
        G: Capabilities<'id, Peer = P, Owner = Route<'p, P, Rc<dyn ClientHook>, H>>,
    {
        self.ready.register(cx.waker());
        if self.is_empty() {
            return Poll::Ready(Ok(Progress::Idle));
        }
        for _ in 0..budget.get() {
            let mut ready = if self.prefer_ready {
                self.ready.pop()
            } else {
                None
            };
            self.prefer_ready = !self.prefer_ready;
            if ready.is_none() {
                if let Some(event) = self
                    .listener
                    .as_ref()
                    .and_then(|listener| graph.next_event(listener))
                {
                    let index = event.owner;
                    if self
                        .entries
                        .get(index)
                        .and_then(Option::as_ref)
                        .is_some_and(|entry| {
                            entry.key == event.client
                                && entry.subscription.key() == event.subscription
                        })
                    {
                        self.remove(index, graph);
                        return Poll::Ready(Ok(Progress::Abandoned));
                    }
                    continue;
                }
                ready = self.ready.pop();
            }
            let Some(ready) = ready else {
                return Poll::Pending;
            };
            let index = ready.index();
            if let Some(entry) = self.entries[index].as_mut() {
                let source = match graph.retain_key(entry.key) {
                    Ok(source) => source,
                    Err(clients::Error::Stale) => {
                        self.remove(index, graph);
                        return Poll::Ready(Ok(Progress::Abandoned));
                    }
                    Err(error) => {
                        self.ready.push(index);
                        return Poll::Ready(Err(graph_error(error)));
                    }
                };
                let mut operation_cx = Context::from_waker(&entry.wake);
                let progress = advance(
                    entry.key,
                    &mut entry.state,
                    &source,
                    graph,
                    &mut operation_cx,
                    self.introductions.as_ref(),
                );
                graph.release(source);
                match progress {
                    Ok(Some(Progress::Settled)) => {
                        self.remove(index, graph);
                        // Publish full failure details before ordinary collection
                        // can signal an awaiting generated client.
                        hooks.collect_with_metadata(graph, metadata);
                        return Poll::Ready(Ok(Progress::Settled));
                    }
                    Ok(Some(progress)) => {
                        self.ready.push(index);
                        return Poll::Ready(Ok(progress));
                    }
                    Err(error) => {
                        self.ready.push(index);
                        return Poll::Ready(Err(error));
                    }
                    Ok(None) => (),
                }
            }
            if self.ready.is_empty()
                && !self
                    .listener
                    .as_ref()
                    .is_some_and(|listener| graph.has_events(listener))
            {
                return Poll::Pending;
            }
        }
        Poll::Ready(Ok(Progress::Yield))
    }
    fn remove<G: Capabilities<'id>>(&mut self, index: usize, graph: &mut G) {
        if let Some(entry) = self.entries[index].take() {
            graph.unsubscribe(entry.subscription);
            self.ready.remove(index);
            self.live -= 1;
            self.first_free = self.first_free.min(index);
        }
    }
    pub fn terminate<G: Capabilities<'id>>(mut self, graph: &mut G) {
        if let Some(listener) = self.listener.take() {
            graph.close_listener(listener);
        }
    }
}

fn advance<'p, 'id, P: Copy + Eq, const H: usize, G>(
    key: clients::Key<'id>,
    state: &mut State,
    source: &Client<'id>,
    graph: &mut G,
    cx: &mut Context<'_>,
    introductions: Option<&coordinator::multiparty::introduction::Policy>,
) -> capnp::Result<Option<Progress>>
where
    G: Capabilities<'id, Peer = P, Owner = Route<'p, P, Rc<dyn ClientHook>, H>>,
{
    match state {
        State::Waiting { future, .. } => {
            let Poll::Ready(result) = Pin::new(future).poll(cx) else {
                return Ok(None);
            };
            *state = match result {
                Err(error) => State::Ready(Err(error)),
                Ok(hook) => {
                    let hook: Rc<dyn ClientHook> = Rc::from(hook);
                    match hook.when_more_resolved() {
                        Some(future) => State::Waiting {
                            _hook: hook,
                            future,
                        },
                        None => State::Ready(Ok(hook)),
                    }
                }
            };
            Ok(Some(Progress::Advanced))
        }
        State::Ready(result) => {
            let replacement = match result {
                Ok(hook) => {
                    if let Some(vine) = introductions.and_then(|policy| policy(&**hook)) {
                        graph
                            .endpoint(Route::Vine(vine), Location::Local)
                            .map_err(|e| graph_error(e.error))?
                    } else {
                        let Destination::Endpoint { key, .. } =
                            graph.inspect(source).map_err(graph_error)?
                        else {
                            return Err(Error::failed(
                                "foreign promise lost its original proxy".into(),
                            ));
                        };
                        graph.retain_key(key).map_err(graph_error)?
                    }
                }
                Err(error) => graph
                    .endpoint_with(Location::Broken, || {
                        Route::Capability(Endpoint::Failed {
                            kind: error.kind,
                            owner: Rc::from(crate::broken::new_cap(error.clone())),
                        })
                    })
                    .map_err(graph_error)?,
            };
            let mut replacement = Some(replacement);
            match graph.resolve(key, &mut replacement, clients::Source::Direct) {
                Ok(None) => Ok(Some(Progress::Settled)),
                Ok(Some(_)) => unreachable!("local proxy resolution requires no remote barrier"),
                Err(error) => {
                    graph.release(replacement.expect("rejected resolution retains replacement"));
                    Err(graph_error(error))
                }
            }
        }
    }
}
pub(super) fn retain_published<'id>(
    graph: &mut impl Capabilities<'id>,
    hook: &dyn ClientHook,
) -> capnp::Result<Option<Client<'id>>> {
    graph
        .retain_exported(|| hook_identity(hook))
        .map_err(graph_error)
}

fn graph_error(error: clients::Error) -> Error {
    match error {
        clients::Error::Full
        | clients::Error::ReferencesExhausted
        | clients::Error::GenerationsExhausted => Error::overloaded(error.to_string()),
        _ => wire_error(error),
    }
}

/// Only concrete private clients provide canonical outgoing identities. An
/// opaque hook can preserve get_ptr/get_brand diagnostics without allowing its
/// interception policy to be bypassed by a previously exported inner client.
pub(crate) fn export_identity<P, const H: usize>(
    route: &Route<'_, P, Rc<dyn ClientHook>, H>,
) -> Option<capntproto_rpc_engine::exports::Identity> {
    match route {
        Route::Capability(Endpoint::Local(hook)) => hook_identity(&**hook),
        _ => None,
    }
}
fn hook_identity(hook: &dyn ClientHook) -> Option<capntproto_rpc_engine::exports::Identity> {
    let any = hook.as_any()?;
    let identity = if let Some(local) = any.downcast_ref::<crate::local::Identity>() {
        (1, local.get())
    } else {
        let queued = any.downcast_ref::<crate::queued::Client>()?;
        (2, queued.get_ptr())
    };
    // Match legacy's normalization before exports_by_cap lookup, including
    // a queued client which has already resolved back to a local server.
    if let Some(next) = hook.get_resolved() {
        return hook_identity(&*next);
    }
    Some(capntproto_rpc_engine::exports::Identity::for_owner(
        identity.0, identity.1,
    ))
}
/// Metadata from the original local hook, including imported failure details.
/// Descriptor ownership is queried through the policy hook, never its inner cap.
pub fn local_metadata(hook: &Rc<dyn ClientHook>) -> EndpointMetadata {
    EndpointMetadata {
        introduction: None,
        failure: hook
            .as_any()
            .and_then(|any| any.downcast_ref::<crate::broken::Client>())
            .map(|hook| hook.error().clone()),
        #[cfg(unix)]
        fd: hook.get_fd(),
    }
}
/// Local endpoint metadata. For remote descriptor ownership use
/// `ClientHooks::collect_wire` with the connection's import metadata provider.
pub fn metadata<P, const H: usize>(
    route: &Route<'_, P, Rc<dyn ClientHook>, H>,
) -> EndpointMetadata {
    match route {
        Route::Capability(Endpoint::Local(hook) | Endpoint::Failed { owner: hook, .. })
        | Route::Vine(hook) => local_metadata(hook),
        Route::Capability(Endpoint::Broken(kind)) => EndpointMetadata {
            failure: Some(Error::from_kind(*kind)),
            ..Default::default()
        },
        _ => EndpointMetadata::default(),
    }
}

#[cfg(test)]
mod tests;
