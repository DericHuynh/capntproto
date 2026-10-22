//! Ordered resolution against a caller-authenticated cross-connection result.
use super::*;
use crate::owned::connection::LocalResult;
impl<'id, P: Copy + Eq, const PATH: usize> Bindings<'_, 'id, P, PATH> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn poll_handoff<'pool, S, G, const CAPS: usize>(
        &mut self,
        index: usize,
        question: questions::Key<'id>,
        questions: &mut questions::Questions<'id, '_>,
        result: Result<&LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
        messages: &messages::Messages<'pool, '_, '_>,
        graph: &mut G,
        retain_route: bool,
    ) -> Result<Progress<'id>, Error>
    where
        G: clients::Capabilities<'id, Owner = Route<'id, P, S, PATH>, Peer = P>,
    {
        let slot = self.slots.get_mut(index).ok_or(Error::Capacity)?;
        let Some(entry) = slot
            .entry
            .as_ref()
            .filter(|entry| entry.question == question)
        else {
            return Ok(Progress::Unrelated);
        };
        let observed = match graph.retain_key(entry.client) {
            Ok(client) => client,
            Err(clients::Error::Stale) => {
                self.finish(index, questions, graph);
                return Ok(Progress::Abandoned);
            }
            Err(error) => return Err(Error::Graph(error)),
        };
        let path = pipeline::Path::new(&entry.fields[..entry.length]);
        let candidate = (|| {
            let kind = match result {
                // Cross-connection adoption does not authorize an ordinary
                // sender loopback through a third peer. Keep advertised routes.
                // Late observers never advertised a route. Their unresolved
                // local promise must adopt the result, not snapshot itself.
                Ok(_) if retain_route && entry.original_route => {
                    return graph
                        .snapshot(&observed)
                        .map(|client| (client, clients::Source::Direct))
                        .map_err(Error::Graph)
                }
                Ok(result) => match result
                    .with(messages, |payload| match payload.pipeline(path) {
                        Ok(client) => graph.retain_key(client.key()).map(Ok).map_err(Error::Graph),
                        Err(owned::Error::Wire(kind)) => Ok(Err(kind)),
                        Err(_) => Ok(Err(capnp::ErrorKind::Failed)),
                    })
                    .map_err(Error::Input)??
                {
                    Ok(client) => return Ok((client, clients::Source::Reflected)),
                    Err(kind) => kind,
                },
                Err(kind) => kind,
            };
            graph
                .endpoint_with(clients::Location::Broken, || Route::broken(kind))
                .map(|client| (client, clients::Source::Direct))
                .map_err(Error::Graph)
        })();
        let (client, source) = match candidate {
            Ok(value) => value,
            Err(error) => {
                graph.release(observed);
                return Err(error);
            }
        };
        let mut replacement = Some(client);
        let resolved = graph.resolve(observed.key(), &mut replacement, source);
        graph.release(observed);
        match resolved {
            Ok(barrier) => {
                self.finish(index, questions, graph);
                Ok(Progress::Resolved(barrier))
            }
            Err(error) => {
                graph.release(replacement.expect("unaccepted handoff"));
                Err(Error::Graph(error))
            }
        }
    }
}
