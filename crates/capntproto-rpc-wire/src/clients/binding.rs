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

impl<'graph, 'imports, P: Copy + Eq> Imports<'_, 'graph, 'imports, P> {
    /// Bind an admitted capability from parameters, results, or Resolve. The
    /// original receipt is preserved; the returned client has independent life.
    #[allow(clippy::too_many_arguments)]
    pub fn bind_capability<'e, 'a, I, A, const PATH: usize, G>(
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
