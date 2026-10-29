use super::*;
use crate::{owned::pipelines, pipeline, reception::full::Capability};
use capntproto_rpc_engine::{answers, exports};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindError {
    Imports(super::Error),
    Pipeline(pipelines::Error),
    Path(pipeline::Error),
    Graph(clients::Error),
}
impl core::fmt::Display for BindError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "capability route binding: {self:?}")
    }
}
impl core::error::Error for BindError {}

/// A descriptor's owning graph route and its protocol ordering provenance.
/// Remote and reflected sources cannot accidentally use the same resolution
/// policy. On rejected resolution, discard the fresh pipeline observer as well.
#[must_use = "retain the bound client or explicitly discard it"]
pub struct Bound<'graph> {
    client: clients::Client<'graph>,
    source: clients::Source,
    observer: bool,
}
impl<'graph> Bound<'graph> {
    pub fn into_client(self) -> clients::Client<'graph> {
        self.client
    }
    pub(crate) fn parts(self) -> (clients::Client<'graph>, clients::Source, bool) {
        (self.client, self.source, self.observer)
    }
}

/// Admitted import bindings for one graph and import ownership domain.
/// Implementations are sealed; connection drivers use the import ledger itself,
/// while independently branded low-level graphs use an explicit binding map.
pub trait ImportBindings<'graph, 'imports, P: Copy + Eq>: super::sealed::Sealed {
    fn peer(&self) -> P;
    fn bind<I, G>(
        &mut self,
        graph: &mut G,
        imports: &mut imports::Imports<'imports, '_, I>,
        local: &imports::Local<'imports>,
    ) -> Result<clients::Client<'graph>, super::Error>
    where
        G: clients::Capabilities<'graph, Peer = P>,
        G::Owner: EndpointOwner<'imports, P>;
    fn resolution_target<I>(
        &self,
        key: imports::Key<'imports>,
        imports: &imports::Imports<'imports, '_, I>,
    ) -> Option<clients::Key<'graph>>;

    /// Freeze the exact receiverHosted route without following a promise.
    fn freeze_import<I, G>(
        &self,
        graph: &mut G,
        imports: &mut imports::Imports<'imports, '_, I>,
        local: &imports::Local<'imports>,
    ) -> Result<clients::Client<'graph>, super::Error>
    where
        G: clients::Capabilities<'graph, Peer = P>,
        G::Owner: EndpointOwner<'imports, P>,
    {
        super::import_client(graph, imports, local, self.peer(), false)
    }

    /// Bind an admitted capability from parameters, results, or Resolve. The
    /// original receipt is preserved; the returned client has independent life.
    #[allow(clippy::too_many_arguments)]
    fn bind_capability<'e, 'a, I, A, const PATH: usize, G>(
        &mut self,
        capability: Capability<'_, '_, 'imports, 'e, 'a>,
        graph: &mut G,
        imports: &mut imports::Imports<'imports, '_, I>,
        exports: &exports::Exports<'e, '_, clients::Client<'graph>>,
        answers: &mut answers::Answers<'a, '_, A>,
        pipelines: &mut pipelines::Bindings<'_, 'graph, 'a, PATH>,
    ) -> Result<Bound<'graph>, BindError>
    where
        G: clients::Capabilities<'graph, Peer = P>,
        G::Owner: EndpointOwner<'imports, P>,
    {
        let (client, source, observer) = match capability {
            Capability::Remote(local) => (
                self.bind(graph, imports, local)
                    .map_err(BindError::Imports)?,
                clients::Source::Direct,
                false,
            ),
            Capability::Local(lease) => (
                super::reflected(graph, exports, lease).map_err(BindError::Graph)?,
                clients::Source::Reflected,
                false,
            ),
            Capability::Promised { answer, path } => {
                let mut fields = pipelines
                    .path_capacity
                    .buffer::<_, PATH>(
                        pipeline::Path::required(path).map_err(BindError::Path)?,
                        || 0,
                    )
                    .ok_or(BindError::Path(pipeline::Error::Capacity))?;
                let path = pipeline::Path::decode(path, &mut fields).map_err(BindError::Path)?;
                (
                    pipelines
                        .bind(graph, answers, answer, path)
                        .map_err(BindError::Pipeline)?,
                    clients::Source::Reflected,
                    true,
                )
            }
        };
        Ok(Bound {
            client,
            source,
            observer,
        })
    }
}

impl<P> super::sealed::Sealed for Imports<'_, '_, '_, P> {}
impl<'graph, 'imports, P: Copy + Eq> ImportBindings<'graph, 'imports, P>
    for Imports<'_, 'graph, 'imports, P>
{
    fn peer(&self) -> P {
        self.peer
    }
    fn bind<I, G>(
        &mut self,
        graph: &mut G,
        imports: &mut imports::Imports<'imports, '_, I>,
        local: &imports::Local<'imports>,
    ) -> Result<clients::Client<'graph>, super::Error>
    where
        G: clients::Capabilities<'graph, Peer = P>,
        G::Owner: EndpointOwner<'imports, P>,
    {
        Imports::bind(self, graph, imports, local)
    }
    fn resolution_target<I>(
        &self,
        key: imports::Key<'imports>,
        _: &imports::Imports<'imports, '_, I>,
    ) -> Option<clients::Key<'graph>> {
        Imports::resolution_target(self, key)
    }
}

/// A driver's graph and import ledger share one affine connection domain.
/// Their weak link lives in the import entry; there is no second table/budget.
pub(crate) struct ConnectionImports<P> {
    peer: P,
}
impl<P> ConnectionImports<P> {
    pub(crate) fn new(peer: P) -> Self {
        Self { peer }
    }
}
impl<P> super::sealed::Sealed for ConnectionImports<P> {}
impl<'id, P: Copy + Eq> ImportBindings<'id, 'id, P> for ConnectionImports<P> {
    fn peer(&self) -> P {
        self.peer
    }
    fn bind<I, G>(
        &mut self,
        graph: &mut G,
        imports: &mut imports::Imports<'id, '_, I>,
        local: &imports::Local<'id>,
    ) -> Result<clients::Client<'id>, super::Error>
    where
        G: clients::Capabilities<'id, Peer = P>,
        G::Owner: EndpointOwner<'id, P>,
    {
        if let Some(key) = imports.client_binding(local.key()) {
            match graph.retain_key(key) {
                Ok(client) => return Ok(client),
                Err(clients::Error::Stale) => (),
                Err(error) => return Err(super::Error::Graph(error)),
            }
        }
        let pending = imports.promise_state(local) == promise::ImportState::Pending;
        let client = super::import_client(graph, imports, local, self.peer, pending)?;
        imports.bind_client(local, &client);
        Ok(client)
    }
    fn resolution_target<I>(
        &self,
        key: imports::Key<'id>,
        imports: &imports::Imports<'id, '_, I>,
    ) -> Option<clients::Key<'id>> {
        imports.client_binding(key)
    }
}
