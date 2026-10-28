//! One generative ownership domain for a connection's caller-backed ledgers.
//! The scope composes existing transactional tables without another allocator,
//! executor or global identity counter. Wire/message admission lives in rpc-wire.
#![forbid(unsafe_code)]
use crate::{answers, exports, imports, questions};
mod graph;
mod terminated;
pub use graph::{
    with_domain, with_domain_async, Domain, GraphConnection, GraphError, GraphStorage,
};
#[cfg(feature = "alloc")]
pub use graph::{GrowingStorage, Growth};
pub use terminated::Terminated;

pub struct Storage<'storage, I, E, A> {
    pub imports: &'storage mut [imports::Slot<I>],
    pub exports: &'storage mut [exports::Slot<E>],
    pub questions: &'storage mut [questions::Slot],
    /// Separate high-ID capacity. An empty slice disables no-Return calls.
    pub pipeline_questions: &'storage mut [questions::PipelineSlot],
    pub answers: &'storage mut [answers::Slot<A>],
}

/// Import, export, question and answer roles remain distinct even though their
/// handles share one connection brand. A handle cannot escape or enter another
/// connection scope. Split borrows allow joint input/output admission.
pub struct Connection<'id, 'storage, I, E, A> {
    pub imports: imports::Imports<'id, 'storage, I>,
    pub exports: exports::Exports<'id, 'storage, E>,
    pub questions: questions::Questions<'id, 'storage>,
    pub answers: answers::Answers<'id, 'storage, A>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Imports(imports::Error),
    Exports(exports::Error),
    Questions(questions::Error),
    Answers(answers::Error),
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "connection storage: {self:?}")
    }
}
impl core::error::Error for Error {}

fn new_scope<'id, I, E, A>(
    storage: Storage<'_, I, E, A>,
) -> Result<Connection<'id, '_, I, E, A>, Error> {
    new_scope_with_adoptions(storage, &mut [])
}
fn new_scope_with_adoptions<'id, 's, I, E, A>(
    storage: Storage<'s, I, E, A>,
    adopted: &'s mut [questions::AdoptedSlot],
) -> Result<Connection<'id, 's, I, E, A>, Error> {
    Ok(Connection {
        imports: imports::new_scope(storage.imports).map_err(Error::Imports)?,
        exports: exports::new_scope(storage.exports).map_err(Error::Exports)?,
        questions: questions::new_scope_with_adoptions(
            storage.questions,
            storage.pipeline_questions,
            adopted,
        )
        .map_err(Error::Questions)?,
        answers: answers::new_scope(storage.answers).map_err(Error::Answers)?,
    })
}
/// Admit callee-selected third-party answers into separate fixed storage.
/// Ordinary calls keep their own capacity and cannot consume adopted slots.
pub fn with_connection_adoptions<'s, I, E, A, R>(
    storage: Storage<'s, I, E, A>,
    adopted: &'s mut [questions::AdoptedSlot],
    f: impl for<'id> FnOnce(&mut Connection<'id, 's, I, E, A>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope_with_adoptions(storage, adopted)?))
}

/// Ending the scope while work remains is terminal connection teardown. Quiesce
/// IO first; table cleanup drops remaining owners without sending wire messages.
/// IO, time, scheduling, message buffers and output backpressure stay caller-owned.
pub fn with_connection<'storage, I, E, A, R>(
    storage: Storage<'storage, I, E, A>,
    f: impl for<'id> FnOnce(&mut Connection<'id, 'storage, I, E, A>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(storage)?))
}

/// The caller pins and polls this future. Suspending does not allocate or widen
/// the handle domain; dropping the future has the same terminal teardown rule.
pub async fn with_connection_async<'storage, I, E, A, R>(
    storage: Storage<'storage, I, E, A>,
    f: impl for<'id> core::ops::AsyncFnOnce(&mut Connection<'id, 'storage, I, E, A>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(storage)?).await)
}
