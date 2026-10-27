//! Retain direct promised-answer routes in their originating operation.
//! The ordinary graph still owns ordering barriers. No branded route escapes
//! into a hosted hook, and endpoint materialization waits until Return or a
//! general forwarding path actually needs the hosted registry.
use super::*;

struct Projection<'id> {
    endpoint: Rc<Endpoint>,
    client: Client<'id>,
}
#[derive(Default)]
pub(super) struct DirectProjections<'id> {
    first: Option<Projection<'id>>,
    rest: Vec<Projection<'id>>,
}
impl<'id> DirectProjections<'id> {
    pub(super) fn is_empty(&self) -> bool {
        self.first.is_none()
    }
    pub(super) fn insert(
        &mut self,
        endpoint: Rc<Endpoint>,
        client: Client<'id>,
    ) -> Result<usize, Client<'id>> {
        let projection = Projection { endpoint, client };
        if self.first.is_none() {
            self.first = Some(projection);
            Ok(0)
        } else if self.rest.try_reserve(1).is_ok() {
            self.rest.push(projection);
            Ok(self.rest.len())
        } else {
            Err(projection.client)
        }
    }
    pub(super) fn target(&self, endpoint: &Rc<Endpoint>) -> Option<&Client<'id>> {
        let index = endpoint.direct.get()?;
        let projection = if index == 0 {
            self.first.as_ref()?
        } else {
            self.rest.get(index - 1)?
        };
        Rc::ptr_eq(&projection.endpoint, endpoint).then_some(&projection.client)
    }
    pub(super) fn materialize(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        graph: &mut impl Capabilities<'id>,
    ) -> capnp::Result<()> {
        let first = self.first.take();
        let rest = std::mem::take(&mut self.rest);
        let mut result = Ok(());
        for projection in first.into_iter().chain(rest) {
            projection.endpoint.direct.set(None);
            if let Err(error) = &result {
                graph.release(projection.client);
                projection.endpoint.forward_projection(Err(error.clone()));
            } else if let Err(error) =
                hooks.bind_projection(graph, projection.client, projection.endpoint.clone())
            {
                projection.endpoint.forward_projection(Err(error.clone()));
                result = Err(error);
            }
        }
        result
    }
    pub(super) fn release(&mut self, graph: &mut impl Capabilities<'id>) {
        for projection in self.first.take().into_iter().chain(self.rest.drain(..)) {
            projection.endpoint.direct.set(None);
            graph.release(projection.client);
        }
    }
}
