//! Network registration and shutdown ownership shared by hosted RPC drivers.
use crate::{rpc, Bootstrap, Connection, Error};
use capntproto_rpc_engine::completion::shared;
use futures::{future::Shared, Future};
use std::{
    cell::RefCell,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Poll},
};

pub(crate) mod registry;
use registry::{Acquired, Connections};

/// Both hosted drivers use the same network execution owner. Network completion
/// ends the task set; Disconnected remains an ordinary system shutdown.
pub(crate) fn drive_network<V>(
    network: &mut dyn crate::VatNetwork<V>,
    tasks: &mut crate::task_set::TaskSetHandle<Error>,
) {
    register_network(network, tasks, std::future::ready, true);
}

/// A network's writer can finish before a protocol task observes its close
/// receipt. The portable owner waits for registration cleanup before allowing
/// network completion to terminate the task set.
pub(crate) fn drive_network_with_completion<V, F: Future<Output = capnp::Result<()>> + 'static>(
    network: &mut dyn crate::VatNetwork<V>,
    tasks: &mut crate::task_set::TaskSetHandle<Error>,
    complete: impl FnOnce(capnp::Result<()>) -> F + 'static,
) {
    register_network(network, tasks, complete, false);
}

fn register_network<V, F: Future<Output = capnp::Result<()>> + 'static>(
    network: &mut dyn crate::VatNetwork<V>,
    tasks: &mut crate::task_set::TaskSetHandle<Error>,
    complete: impl FnOnce(capnp::Result<()>) -> F + 'static,
    write_last: bool,
) {
    let driving = network.drive_until_shutdown();
    let mut handle = tasks.clone();
    let task = async move {
        let result = match complete(driving.await).await {
            Err(error) if error.kind == capnp::ErrorKind::Disconnected => Ok(()),
            result => result,
        };
        handle.terminate(result);
        Ok(())
    };
    if write_last {
        tasks.add_last(task);
    } else {
        tasks.add(task);
    }
}

pub(crate) struct ConnectionRegistry<VatId: 'static> {
    pub(crate) join_context: Weak<rpc::JoinContext<VatId>>,
    pub(crate) join_network: Option<Rc<dyn crate::multiparty::JoinNetwork<VatId>>>,
    pub(crate) flow_limit: usize,
    pub(crate) outgoing_call_limit: usize,
    pub(crate) trace_encoder: Option<Rc<dyn Fn(&Error) -> String>>,
    registered: Connections<rpc::ConnectionState<VatId>>,
}
impl<VatId> ConnectionRegistry<VatId> {
    pub(crate) fn new() -> Self {
        Self {
            join_context: Weak::new(),
            join_network: None,
            flow_limit: usize::MAX,
            outgoing_call_limit: usize::MAX,
            trace_encoder: None,
            registered: Connections::new(),
        }
    }
    pub(crate) fn is_closing(&self) -> bool {
        self.registered.is_closing()
    }
    pub(crate) fn states(&self) -> Vec<Rc<rpc::ConnectionState<VatId>>> {
        self.registered.states()
    }
    pub(crate) fn detach(
        registry: &Rc<RefCell<Self>>,
        id: usize,
        state: &rpc::ConnectionState<VatId>,
    ) {
        let registered = registry.borrow().registered.clone();
        registered.detach(id, state);
    }
    pub(crate) fn begin_close(
        registry: &Rc<RefCell<Self>>,
    ) -> Vec<Rc<rpc::ConnectionState<VatId>>> {
        let registered = registry.borrow().registered.clone();
        registered.begin_close()
    }
}
impl<VatId> registry::Connection for rpc::ConnectionState<VatId> {
    fn disconnect(&self, error: Error) {
        self.disconnect(error);
    }
}

pub(crate) fn connect<VatId>(
    connections: &Rc<RefCell<ConnectionRegistry<VatId>>>,
    bootstrap: Bootstrap<VatId>,
    connection: Box<dyn Connection<VatId>>,
    mut tasks: crate::task_set::TaskSetHandle<Error>,
) -> capnp::Result<Rc<rpc::ConnectionState<VatId>>> {
    let id = connection.connection_id();
    let (registered, flow, outgoing) = {
        let registry = connections.borrow();
        (
            registry.registered.clone(),
            registry.flow_limit,
            registry.outgoing_call_limit,
        )
    };
    let registration = match registered.acquire(id)? {
        Acquired::Existing(state) => {
            state.set_not_idle();
            return Ok(state);
        }
        Acquired::New(registration) => registration,
    };
    let (sender, receiver) = shared::channel();
    let (driver, state) = rpc::ConnectionState::new(
        id,
        bootstrap,
        connection,
        sender,
        Rc::downgrade(connections),
        tasks.clone(),
        flow,
        outgoing,
    );
    let published = registration.publish(state.clone());
    let retained = state.clone();
    // The affine registration is captured now, not constructed on first poll.
    tasks.add(async move {
        let result = match receiver.await {
            Ok(shutdown) => shutdown.await,
            Err(error) => Err(Error::failed(error.to_string())),
        };
        drop(retained);
        registration.complete(result.clone());
        result
    });
    tasks.add(driver);
    if let Err(error) = published {
        // A transport callback may have started shutdown during construction.
        // Its reservation remains counted through this final transport close.
        state.disconnect(error.clone());
        return Err(error);
    }
    Ok(state)
}

/// Closes every registered connection and waits for final transport shutdown.
/// Multiple observers are independent. Canceled connection tasks report an error
/// instead of leaving a waiter asleep or claiming a successful close.
pub struct Disconnector<VatId: 'static> {
    connections: Rc<RefCell<ConnectionRegistry<VatId>>>,
    closed: Shared<capnp::capability::Promise<(), Error>>,
}
impl<VatId> Disconnector<VatId> {
    pub(crate) fn new(connections: Rc<RefCell<ConnectionRegistry<VatId>>>) -> Self {
        let closed = connections.borrow().registered.closed();
        Self {
            connections,
            closed,
        }
    }
}
impl<VatId: 'static> Future for Disconnector<VatId> {
    type Output = capnp::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let states = ConnectionRegistry::begin_close(&self.connections);
        for state in states {
            state.disconnect(Error::disconnected("client requested disconnect".into()));
        }
        Pin::new(&mut self.closed).poll(cx)
    }
}

#[cfg(test)]
mod tests;
