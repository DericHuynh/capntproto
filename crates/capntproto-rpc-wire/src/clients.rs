//! Bridge admitted import references into the caller-backed capability graph.
//! Aliases share one observed promise; the graph owns one local import reference
//! until its immutable remote endpoint is explicitly drained. The alias map is
//! weak and cannot form a graph/import ownership cycle.
#![forbid(unsafe_code)]
use capntproto_rpc_engine::storage::Storage as SlotStorage;
use capntproto_rpc_engine::{clients, ids::ImportId, imports, promise};
mod binding;
pub(crate) use binding::ConnectionImports;
pub use binding::{BindError, Bound, ImportBindings};
mod typed;
pub use typed::{CallTarget, TypedClient};
pub mod exporting;
pub mod outgoing;
pub use outgoing::{QuestionRoute, Route};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportError {
    Graph(clients::Error),
    Export(capntproto_rpc_engine::exports::Error),
    PendingSource,
}
impl core::fmt::Display for ExportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "graph export admission: {self:?}")
    }
}
impl core::error::Error for ExportError {}

/// Stage an already-known advertised graph route. Future resolution of a remote
/// source cannot silently rewrite this export. Unresolved local sources require
/// an export-resolution observer and are rejected without changing the batch.
pub fn stage<'g, 'e, G: clients::Capabilities<'g>>(
    graph: &mut G,
    batch: &mut capntproto_rpc_engine::exports::Batch<'_, 'e, '_, clients::Client<'g>>,
    client: &clients::Client<'g>,
) -> Result<capntproto_rpc_engine::exports::Grant<'e>, ExportError> {
    let (frozen, promise) = graph
        .snapshot_for_export(client)
        .map_err(ExportError::Graph)?;
    if promise {
        graph.release(frozen);
        return Err(ExportError::PendingSource);
    }
    match batch.grant_canonical(frozen.key()) {
        Ok(Some(grant)) => {
            graph.release(frozen);
            return Ok(grant);
        }
        Err(error) => {
            graph.release(frozen);
            return Err(ExportError::Export(error));
        }
        Ok(None) => (),
    }
    let result = batch.export_canonical(frozen.key(), frozen);
    result.map_err(|rejected| {
        graph.release(rejected.value);
        ExportError::Export(rejected.error)
    })
}

mod sealed {
    pub trait Sealed {}
}
/// Wire-created endpoints used by the graph adapter. Sealed implementations
/// preserve the imported receipt and the remote/broken route distinction.
pub trait EndpointOwner<'i, P>: sealed::Sealed + Sized {
    fn imported(remote: Remote<'i, P>) -> Self;
    fn broken(error: capnp::ErrorKind) -> Self;
    fn into_import(self) -> Result<Remote<'i, P>, Self>;
    fn exception(&self) -> Option<capnp::ErrorKind>;
}

#[derive(Debug)]
pub struct Remote<'imports, P> {
    local: imports::Local<'imports>,
    wire: ImportId,
    peer: P,
}
impl<'i, P: Copy> Remote<'i, P> {
    pub fn id(&self) -> ImportId {
        self.wire
    }
    pub fn peer(&self) -> P {
        self.peer
    }
    pub fn reference(&self) -> &imports::Local<'i> {
        &self.local
    }
}
#[derive(Debug)]
pub enum Endpoint<'imports, P, S> {
    Imported(Remote<'imports, P>),
    Local(S),
    Broken(capnp::ErrorKind),
    /// A failed application endpoint retaining caller-owned diagnostics. The
    /// owner handles local calls; protocol observers still publish an exception.
    Failed {
        kind: capnp::ErrorKind,
        owner: S,
    },
}
impl<P, S> sealed::Sealed for Endpoint<'_, P, S> {}
impl<'i, P, S> EndpointOwner<'i, P> for Endpoint<'i, P, S> {
    fn exception(&self) -> Option<capnp::ErrorKind> {
        match self {
            Self::Broken(error) | Self::Failed { kind: error, .. } => Some(*error),
            _ => None,
        }
    }
    fn imported(remote: Remote<'i, P>) -> Self {
        Self::Imported(remote)
    }
    fn broken(error: capnp::ErrorKind) -> Self {
        Self::Broken(error)
    }
    fn into_import(self) -> Result<Remote<'i, P>, Self> {
        match self {
            Self::Imported(remote) => Ok(remote),
            other => Err(other),
        }
    }
}
impl<'i, P, S> Endpoint<'i, P, S> {
    /// Drain after graph borrows end. Application servers are returned, never
    /// destructed by an import/graph transition. Wire Release remains queued in
    /// its import slot until the connection accepts the actual control frame.
    pub fn release<I>(self, imports: &mut imports::Imports<'i, '_, I>) -> Option<S> {
        match self {
            Self::Imported(remote) => {
                imports.release(remote.local);
                None
            }
            Self::Local(server) | Self::Failed { owner: server, .. } => Some(server),
            Self::Broken(_) => None,
        }
    }
}
pub struct ImportSlot<'graph, 'imports> {
    link: Option<(imports::Key<'imports>, clients::Key<'graph>)>,
}
impl ImportSlot<'_, '_> {
    pub const EMPTY: Self = Self { link: None };
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    Full,
    Imports(imports::Error),
    Graph(clients::Error),
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "wire capability binding: {self:?}")
    }
}
impl core::error::Error for Error {}

/// These slots are declared within the graph scope, which itself can be nested
/// inside the import scope. Each map belongs to one peer/import table. User code
/// cannot construct its keys or move them into another graph/import domain.
pub struct Imports<'slots, 'graph, 'imports, P> {
    slots: SlotStorage<'slots, ImportSlot<'graph, 'imports>>,
    peer: P,
}
impl<'slots, 'graph, 'imports, P: Copy + Eq> Imports<'slots, 'graph, 'imports, P> {
    /// Retain the exact wire import endpoint for a receiverHosted descriptor.
    /// This deliberately does not follow the observed promise's resolution:
    /// an already-advertised reflected route must keep its original ordering.
    pub fn freeze_import<I, G>(
        &self,
        graph: &mut G,
        imports: &mut imports::Imports<'imports, '_, I>,
        local: &imports::Local<'imports>,
    ) -> Result<clients::Client<'graph>, Error>
    where
        G: clients::Capabilities<'graph, Peer = P>,
        G::Owner: EndpointOwner<'imports, P>,
    {
        import_client(graph, imports, local, self.peer, false)
    }
    pub fn new(slots: &'slots mut [ImportSlot<'graph, 'imports>], peer: P) -> Result<Self, Error> {
        Self::from_storage(SlotStorage::borrowed(slots), peer)
    }
    pub(crate) fn from_storage(
        slots: SlotStorage<'slots, ImportSlot<'graph, 'imports>>,
        peer: P,
    ) -> Result<Self, Error> {
        if slots.iter().any(|s| s.link.is_some()) {
            return Err(Error::NonemptyStorage);
        }
        Ok(Self { slots, peer })
    }
    /// Obtain an owning callable route from an already admitted descriptor.
    /// Resource rejection leaves the receipt usable for retry. The import
    /// ledger decides pending status, including senderHosted promise aliases.
    pub fn bind<I, G>(
        &mut self,
        graph: &mut G,
        imports: &mut imports::Imports<'imports, '_, I>,
        local: &imports::Local<'imports>,
    ) -> Result<clients::Client<'graph>, Error>
    where
        G: clients::Capabilities<'graph, Peer = P>,
        G::Owner: EndpointOwner<'imports, P>,
    {
        let matching = self
            .slots
            .iter()
            .position(|s| s.link.is_some_and(|(key, _)| key == local.key()));
        if let Some(index) = matching {
            match graph.retain_key(self.slots[index].link.unwrap().1) {
                Ok(client) => return Ok(client),
                Err(clients::Error::Stale) => (),
                Err(e) => return Err(Error::Graph(e)),
            }
        }
        let slot = matching
            .or_else(|| {
                self.slots.vacant(
                    |_, s| s.link.is_none_or(|(key, _)| imports.resolve(key).is_none()),
                    || ImportSlot::EMPTY,
                )
            })
            .ok_or(Error::Full)?;
        let pending = imports.promise_state(local) == promise::ImportState::Pending;
        let client = import_client(graph, imports, local, self.peer, pending)?;
        self.slots[slot].link = Some((local.key(), client.key()));
        Ok(client)
    }
    /// Locate the observed promise for an admitted Resolve. This non-owning key
    /// must be checked again by graph resolution; cancellation may have retired
    /// it while the wire import still retains a late-resolution obligation.
    pub fn resolution_target(
        &self,
        import: imports::Key<'imports>,
    ) -> Option<clients::Key<'graph>> {
        self.slots
            .iter()
            .find_map(|s| s.link.filter(|(key, _)| *key == import).map(|(_, key)| key))
    }
}
#[cfg(test)]
mod tests;

/// Retain a reflected receiverHosted capability independently of its export
/// lease. A final peer Release can retire the export without revoking this local
/// client. Preserve Source::Reflected when resolving a remote promise to it.
pub fn reflected<'g, 'e, G: clients::Capabilities<'g>>(
    graph: &mut G,
    exports: &capntproto_rpc_engine::exports::Exports<'e, '_, clients::Client<'g>>,
    lease: &capntproto_rpc_engine::exports::Lease<'e>,
) -> Result<clients::Client<'g>, clients::Error> {
    graph.retain_key(exports.get(lease).key())
}

fn import_client<'g, 'i, P: Copy + Eq, I, G>(
    graph: &mut G,
    imports: &mut imports::Imports<'i, '_, I>,
    local: &imports::Local<'i>,
    peer: P,
    pending: bool,
) -> Result<clients::Client<'g>, Error>
where
    G: clients::Capabilities<'g, Peer = P>,
    G::Owner: EndpointOwner<'i, P>,
{
    let retained = imports.retain(local).map_err(Error::Imports)?;
    let endpoint = G::Owner::imported(Remote {
        wire: imports.wire_id(local),
        peer,
        local: retained,
    });
    let client = if pending {
        graph.imported_promise(endpoint, peer)
    } else {
        graph.endpoint(endpoint, clients::Location::Remote(peer))
    };
    let client = match client {
        Ok(client) => client,
        Err(rejected) => {
            // The only rejected owner is the one import just retained.
            // Recover its receipt through the sealed owner implementation.
            let remote = rejected
                .value
                .into_import()
                .unwrap_or_else(|_| unreachable!("sealed imported owner"));
            imports.release(remote.local);
            return Err(Error::Graph(rejected.error));
        }
    };
    Ok(client)
}
