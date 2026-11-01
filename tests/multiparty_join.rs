//! Local authority equality regressions for the legacy RPC implementation.
use capnp::{capability::Promise, Error};
use capntproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
#[path = "multiparty_join/equality.rs"]
mod equality;
#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;
use support::driver::System as RpcSystem;
struct JoinBoundary {
    allowed: bool,
    joins: Cell<u32>,
    calls: Cell<u32>,
    blocked: Cell<bool>,
}
impl capnp_rpc::membrane::Policy for JoinBoundary {
    fn allow_join(
        &self,
        direction: capnp_rpc::membrane::Direction,
        targets: &[capnp::capability::Client],
    ) -> capnp::Result<bool> {
        assert_eq!(direction, capnp_rpc::membrane::Direction::Inbound);
        assert_eq!(targets.len(), 2);
        self.joins.set(self.joins.get() + 1);
        Ok(self.allowed)
    }
    fn call(
        &self,
        _: capnp_rpc::membrane::Direction,
        _: u64,
        _: u16,
        _: &capnp::capability::Client,
    ) -> capnp::Result<Option<capnp::capability::Client>> {
        self.calls.set(self.calls.get() + 1);
        if self.blocked.get() {
            Err(Error::failed("joined Native capability blocked".into()))
        } else {
            Ok(None)
        }
    }
}
