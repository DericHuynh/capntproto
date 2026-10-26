use crate::rpc_examples_capnp::node;
use capntproto::{capnp, capnp_rpc, rpc::Connection};
use std::rc::Rc;

pub struct Node {
    depth: u32,
}
impl node::Server for Node {
    fn value(
        self: Rc<Self>,
        context: node::ValueContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(move |_, mut results| {
            results.set_depth(self.depth);
            Ok(())
        })
    }

    fn child(
        self: Rc<Self>,
        context: node::ChildContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(move |_, mut results| {
            let depth = self
                .depth
                .checked_add(1)
                .ok_or_else(|| capnp::Error::overloaded("node depth exhausted".into()))?;
            results.set_node(capnp_rpc::new_client(Node { depth }));
            Ok(())
        })
    }
}

/// Keep both owners alive: capabilities do not keep the connection task alive.
pub fn connect() -> (Connection, Connection, node::Client) {
    use capnp_rpc::rpc_twoparty_capnp::Side;
    let service: node::Client = capnp_rpc::new_client(Node { depth: 0 });
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    let server = Connection::new(server_io, Some(service.client), Side::Server);
    let client = Connection::new(client_io, None, Side::Client);
    let node = client.bootstrap();
    (server, client, node)
}
