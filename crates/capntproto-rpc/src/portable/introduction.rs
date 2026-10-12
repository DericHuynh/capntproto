//! Type-erased hosted network policy. Only peers in this system's registry may
//! introduce one another; transport callbacks authenticate every opaque token.
use super::*;
use capnp::{any_pointer, private::capability::ClientHook};
use coordinator::multiparty::introduction as policy;
type Body = capnp::message::Builder<capnp::message::HeapAllocator>;

pub(super) struct Network<V: 'static> {
    pub connection: vat::Shared<V>,
    pub connections: Weak<Connections<State>>,
    pub context: Weak<ContextOwner<V>>,
}
impl<V: 'static> policy::Network for Network<V> {
    fn supports_answers(&self) -> bool {
        self.connection.borrow().supports_third_party_answers()
    }
    fn tail_destination(
        &self,
        contact: any_pointer::Reader<'_>,
    ) -> capnp::Result<coordinator::multiparty::tail::Destination> {
        let connections = self.connections.upgrade().ok_or_else(closing)?;
        let context = self.context.upgrade().ok_or_else(closing)?;
        if connections.is_closing() {
            return Err(closing());
        }
        let mut completion = Body::new_default();
        let connection = self
            .connection
            .borrow_mut()
            .connect_to_introduced(contact, completion.init_root())?;
        if let Some(connection) = connection {
            let state = super::connection::connect(&connections, &context, connection)?;
            (state.idle)(false);
            state
                .network_controls
                .prepare_adoption(completion.get_root_as_reader()?)
        } else {
            let completing = self
                .connection
                .borrow_mut()
                .complete_third_party_local(completion.get_root_as_reader()?);
            let deadline = self.connection.borrow().third_party_answer_timeout();
            Ok(coordinator::multiparty::tail::Destination::Local(Box::new(
                move |task| {
                    context
                        .tasks
                        .try_add(async move {
                            // Setup is bounded by the transport deadline. Once adopted,
                            // the answer/pipeline owns work independently of the relay.
                            match futures::future::select(completing, deadline).await {
                                futures::future::Either::Left((Ok(exchange), _)) => {
                                    exchange.adopt(task.answer())?;
                                }
                                futures::future::Either::Left((Err(error), _)) => {
                                    task.results.fail(error)
                                }
                                futures::future::Either::Right(_) => task
                                    .results
                                    .fail(Error::disconnected("local tail setup timed out".into())),
                            }
                            Ok(())
                        })
                        .map_err(|_| closing())
                },
            )))
        }
    }
    fn domain(&self) -> usize {
        self.connections.as_ptr() as usize
    }
    fn peer(&self) -> Box<dyn std::any::Any> {
        Box::new(self.connection.borrow().get_peer_vat_id())
    }
    fn introduce(&self, peer: Box<dyn std::any::Any>) -> capnp::Result<Option<(Body, Body)>> {
        let peer = *peer
            .downcast::<V>()
            .map_err(|_| Error::failed("foreign vat identity type".into()))?;
        let connections = self.connections.upgrade().ok_or_else(closing)?;
        if connections.is_closing() {
            return Err(closing());
        }
        let mut contact = Body::new_default();
        let mut recipient = Body::new_default();
        if !self.connection.borrow_mut().introduce_to(
            peer,
            contact.init_root(),
            recipient.init_root(),
        )? {
            return Ok(None);
        }
        Ok(Some((contact, recipient)))
    }
    fn accept(
        &self,
        contact: any_pointer::Reader<'_>,
    ) -> capnp::Result<(Vec<u8>, Promise<Box<dyn ClientHook>, Error>)> {
        let connections = self.connections.upgrade().ok_or_else(closing)?;
        let context = self.context.upgrade().ok_or_else(closing)?;
        if connections.is_closing() {
            return Err(closing());
        }
        let mut completion = Body::new_default();
        let connection = self
            .connection
            .borrow_mut()
            .connect_to_introduced(contact, completion.init_root())?;
        let embargo = self.connection.borrow_mut().generate_embargo_id()?;
        if embargo.is_empty() {
            return Err(Error::failed("network generated empty embargo ID".into()));
        }
        let future = if let Some(connection) = connection {
            let state = super::connection::connect(&connections, &context, connection)?;
            (state.idle)(false);
            let response = state
                .network_controls
                .accept_embargo(completion.get_root_as_reader()?, embargo.clone());
            Promise::from_future(async move { response.await?.get()?.get_as_capability() })
        } else {
            let completing = self
                .connection
                .borrow_mut()
                .complete_third_party_local(completion.get_root_as_reader()?);
            let embargo = embargo.clone();
            Promise::from_future(async move {
                let exchange = completing.await?;
                let future = exchange.accept(Some(embargo));
                drop(exchange);
                future.await
            })
        };
        Ok((embargo, future))
    }
}
