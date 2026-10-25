//! Network contracts for independently routed Join key shares.
use crate::{third_party::ThirdPartyExchange, Connection};
use capnp::{
    any_pointer, capability::Promise, message::Builder, private::capability::ResponseHook, Error,
};
use std::{any::Any, rc::Rc};

/// A network owns the share format, authentication and destination discovery.
/// A single routed share must not authorize the joined capability.
pub trait JoinNetwork<VatId> {
    fn start(&self, count: u16) -> capnp::Result<Box<dyn JoinSession<VatId>>>;
    /// Validate a key part and consume one hop of its forwarding budget.
    fn forward(
        &self,
        part: any_pointer::Reader<'_>,
    ) -> capnp::Result<Builder<capnp::message::HeapAllocator>>;
    /// Register one share at a settled object's identity. The returned guard
    /// remains alive until the corresponding upstream Finish or disconnect.
    fn contribute(
        &self,
        part: any_pointer::Reader<'_>,
        identity: (usize, usize),
        value: Rc<ThirdPartyExchange>,
    ) -> capnp::Result<(Builder<capnp::message::HeapAllocator>, Box<dyn Any>)>;
}

pub trait JoinSession<VatId> {
    fn part(&self, index: u16) -> capnp::Result<Builder<capnp::message::HeapAllocator>>;
    /// Check every response, including proof of possession of all shares for
    /// one object. Return None only for conflicting object/host identities.
    fn finish(
        self: Box<Self>,
        responses: &[Box<dyn ResponseHook>],
    ) -> capnp::Result<Option<JoinDestination<VatId>>>;
}

pub enum JoinDestination<VatId> {
    Local(Rc<ThirdPartyExchange>),
    Remote {
        connection: Box<dyn Connection<VatId>>,
        completion: Builder<capnp::message::HeapAllocator>,
    },
}

/// Result of a forwarded part; its response owns the downstream Finish.
pub type PartResponse = Promise<Box<dyn ResponseHook>, Error>;
