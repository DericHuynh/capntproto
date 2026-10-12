//! Fixed resources shared by the C++ interoperability example.
use capnp_rpc::{
    bounded::owned::connection::{Driver, FixedStorage},
    engine::connection,
};
pub type Peer<'p, 'id, 's> = Driver<'p, 'id, 's, (), (), u8, (), 4, 4>;
pub fn with_owned_peer_tasks<'p, const TASKS: usize, R>(
    f: impl for<'id, 's> FnOnce(Peer<'p, 'id, 's>) -> R,
) -> R {
    connection::with_domain(|domain| {
        let mut resources = FixedStorage::<(), (), u8, (), 4, 4, TASKS, 12>::new();
        f(resources.bind(domain, 1).unwrap())
    })
}
