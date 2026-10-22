//! Joint graph/ledger identity without a Rust lifetime ownership cycle. Graph
//! endpoints can own imports while exports and answers own graph clients.
use super::*;
use crate::clients;
use core::marker::PhantomData;

/// Single-use authority to initialize exactly one graph and its connection.
/// Declare caller storage inside `with_domain`, then consume this token to bind
/// it. No table borrows are stored inside graph clients or ledger receipts.
#[must_use = "bind caller storage to this domain"]
pub struct Domain<'id> {
    brand: PhantomData<fn(&'id ()) -> &'id ()>,
}

/// Introduce the domain before allocating caller storage. This permits storage
/// element types to contain handles in that same domain, in both directions.
pub fn with_domain<R>(f: impl for<'id> FnOnce(Domain<'id>) -> R) -> R {
    f(Domain { brand: PhantomData })
}

pub async fn with_domain_async<R>(f: impl for<'id> core::ops::AsyncFnOnce(Domain<'id>) -> R) -> R {
    f(Domain { brand: PhantomData }).await
}

pub struct GraphStorage<'id, 'storage, I, A, T, P> {
    pub imports: &'storage mut [imports::Slot<I>],
    pub exports: &'storage mut [exports::Slot<clients::Client<'id>>],
    pub questions: &'storage mut [questions::Slot],
    pub answers: &'storage mut [answers::Slot<A>],
    pub clients: &'storage mut [clients::Slot<T, P>],
    pub notifications: clients::NotificationStorage<'storage>,
}

pub struct GraphConnection<'id, 'storage, I, A, T, P> {
    pub ledgers: Connection<'id, 'storage, I, clients::Client<'id>, A>,
    pub clients: clients::Graph<'id, 'storage, T, P>,
}

/// Fallible allocator-backed storage budget. Capacities count logical slots;
/// the allocator may reserve extra memory, but it cannot increase this budget.
#[cfg(feature = "alloc")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Growth {
    pub initial: usize,
    pub maximum: usize,
}

/// Each ledger grows independently. Adopted questions have a separate budget;
/// dispatch, message buffers and IO remain separate resources.
/// Ordinary IDs keep their wire bounds.
#[cfg(feature = "alloc")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrowingStorage {
    pub imports: Growth,
    pub exports: Growth,
    pub questions: Growth,
    pub answers: Growth,
    pub clients: Growth,
    pub notifications: clients::NotificationGrowth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphError {
    Ledgers(Error),
    Clients(clients::Error),
}
impl core::fmt::Display for GraphError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "connection graph storage: {self:?}")
    }
}
impl core::error::Error for GraphError {}

impl<'id> Domain<'id> {
    /// Bind the same graph and ledger state machines to bounded heap growth.
    /// Constructor failure occurs before admitting any protocol owners.
    #[cfg(feature = "alloc")]
    pub fn bind_growing<'storage, I, A, T, P: Copy + Eq>(
        self,
        storage: GrowingStorage,
        adopted: &'storage mut [questions::AdoptedSlot],
    ) -> Result<GraphConnection<'id, 'storage, I, A, T, P>, GraphError> {
        let questions = questions::new_growing_scope(
            storage.questions.initial,
            storage.questions.maximum,
            adopted,
        )
        .map_err(Error::Questions)
        .map_err(GraphError::Ledgers)?;
        self.bind_growing_with_questions(storage, questions)
    }
    /// Grow adopted questions from their own entry-count budget. Their high
    /// wire IDs cannot enlarge this budget or consume ordinary question slots.
    #[cfg(feature = "alloc")]
    pub fn bind_growing_adoptions<'storage, I, A, T, P: Copy + Eq>(
        self,
        storage: GrowingStorage,
        adopted: Growth,
    ) -> Result<GraphConnection<'id, 'storage, I, A, T, P>, GraphError> {
        let adopted = crate::peer_table::SparseSlots::growing(adopted.initial, adopted.maximum)
            .ok_or(GraphError::Ledgers(Error::Questions(
                questions::Error::Full,
            )))?;
        let questions = questions::new_growing_with_adoptions(
            storage.questions.initial,
            storage.questions.maximum,
            adopted,
        )
        .map_err(Error::Questions)
        .map_err(GraphError::Ledgers)?;
        self.bind_growing_with_questions(storage, questions)
    }
    #[cfg(feature = "alloc")]
    fn bind_growing_with_questions<'storage, I, A, T, P: Copy + Eq>(
        self,
        storage: GrowingStorage,
        questions: questions::Questions<'id, 'storage>,
    ) -> Result<GraphConnection<'id, 'storage, I, A, T, P>, GraphError> {
        let ledgers = Connection {
            imports: imports::new_growing_scope(storage.imports.initial, storage.imports.maximum)
                .map_err(Error::Imports)
                .map_err(GraphError::Ledgers)?,
            exports: exports::new_growing_scope(storage.exports.initial, storage.exports.maximum)
                .map_err(Error::Exports)
                .map_err(GraphError::Ledgers)?,
            questions,
            answers: answers::new_growing_scope(storage.answers.initial, storage.answers.maximum)
                .map_err(Error::Answers)
                .map_err(GraphError::Ledgers)?,
        };
        let clients = clients::new_growing_scope(
            storage.clients.initial,
            storage.clients.maximum,
            storage.notifications,
        )
        .map_err(GraphError::Clients)?;
        Ok(GraphConnection { ledgers, clients })
    }
    /// Bind a growing capability graph to fixed caller-backed ledgers. The
    /// same ownership domain and transitions serve both storage providers.
    /// Allocation/capacity failure happens before any capability can be admitted.
    #[cfg(feature = "alloc")]
    pub fn bind_growing_clients<'storage, I, A, T, P: Copy + Eq>(
        self,
        storage: Storage<'storage, I, clients::Client<'id>, A>,
        adopted: &'storage mut [questions::AdoptedSlot],
        initial: usize,
        maximum: usize,
        notifications: clients::NotificationGrowth,
    ) -> Result<GraphConnection<'id, 'storage, I, A, T, P>, GraphError> {
        let clients = clients::new_growing_scope(initial, maximum, notifications)
            .map_err(GraphError::Clients)?;
        let ledgers = new_scope_with_adoptions(storage, adopted).map_err(GraphError::Ledgers)?;
        Ok(GraphConnection { ledgers, clients })
    }
    /// Binding consumes the only constructor for this domain. Initializing two
    /// graphs with interchangeable keys is therefore structurally impossible.
    /// Ending a nonempty connection remains terminal teardown: quiesce IO first.
    pub fn bind<'storage, I, A, T, P: Copy + Eq>(
        self,
        storage: GraphStorage<'id, 'storage, I, A, T, P>,
    ) -> Result<GraphConnection<'id, 'storage, I, A, T, P>, GraphError> {
        self.bind_with_adoptions(storage, &mut [])
    }
    pub fn bind_with_adoptions<'storage, I, A, T, P: Copy + Eq>(
        self,
        storage: GraphStorage<'id, 'storage, I, A, T, P>,
        adopted: &'storage mut [questions::AdoptedSlot],
    ) -> Result<GraphConnection<'id, 'storage, I, A, T, P>, GraphError> {
        let ledgers = new_scope_with_adoptions(
            Storage {
                imports: storage.imports,
                exports: storage.exports,
                questions: storage.questions,
                answers: storage.answers,
            },
            adopted,
        )
        .map_err(GraphError::Ledgers)?;
        let clients = clients::new_scope(storage.clients, storage.notifications)
            .map_err(GraphError::Clients)?;
        Ok(GraphConnection { ledgers, clients })
    }
}
