use crate::{broken, rpc, RpcSystem, VatNetwork};
use capnp::{capability::FromClientHook, Error};
use std::{cell::RefCell, rc::Weak};

/// Cloneable access to a running RPC system's typed bootstrap capabilities.
///
/// Like [`crate::Joiner`], this is a weak control handle. It neither drives RPC
/// nor extends the system's lifetime. Calls may be pipelined immediately on the
/// returned capability; connection/authentication errors arrive through RPC.
pub struct Bootstrapper<VatId: 'static> {
    pub(crate) network: Weak<RefCell<Box<dyn VatNetwork<VatId>>>>,
    pub(crate) connections: Weak<RefCell<rpc::ConnectionRegistry<VatId>>>,
    pub(crate) context: Weak<rpc::JoinContext<VatId>>,
}

impl<VatId> Clone for Bootstrapper<VatId> {
    fn clone(&self) -> Self {
        Self {
            network: self.network.clone(),
            connections: self.connections.clone(),
            context: self.context.clone(),
        }
    }
}

impl<VatId> Bootstrapper<VatId> {
    /// Connect to a vat, or access the local bootstrap when connecting to self.
    /// Peer-specific bootstrap policy is applied by the owning RPC system.
    pub fn bootstrap<T: FromClientHook>(&self, vat_id: VatId) -> T {
        let (Some(network), Some(connections), Some(context)) = (
            self.network.upgrade(),
            self.connections.upgrade(),
            self.context.upgrade(),
        ) else {
            return T::new(broken::new_cap(Error::disconnected(
                "RPC system was dropped".into(),
            )));
        };
        if connections.borrow().closing {
            return T::new(broken::new_cap(Error::disconnected(
                "RPC system is closing".into(),
            )));
        }
        let connection = network.borrow_mut().connect(vat_id);
        let Some(connection) = connection else {
            return T::new(context.bootstrap.local().unwrap_or_else(broken::new_cap));
        };
        let state = RpcSystem::get_connection_state(
            &connections,
            context.bootstrap.clone(),
            connection,
            context.tasks.clone(),
        );
        T::new(rpc::ConnectionState::bootstrap(&state))
    }
}
