//! An owning resource bundle for applications which choose one common slot bound.
//! The low-level Storage and GraphStorage remain available for unequal capacities.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitializeError {
    Domain(connection::GraphError),
    Driver(Error),
}
impl core::fmt::Display for InitializeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RPC resource initialization: {self:?}")
    }
}
impl core::error::Error for InitializeError {}

/// Own all ledger and dispatch slots for one bounded connection. Message buffers,
/// output queues, completion slots and application futures remain separate caller
/// resources. Declare this bundle inside `connection::with_domain`, then bind it
/// once using that domain's affine constructor. A driver borrows this bundle, so
/// its storage cannot move, be rebound or be dropped while the driver is live.
///
/// `SLOTS` applies to each ledger/dispatch table and must be at least two; one
/// dispatch slot is reserved for protocol progress. `NODES` bounds graph nodes.
/// `ADOPTED` reserves separate callee-selected third-party question slots.
/// `LEDGERS` defaults to `SLOTS`; zero selects optional growing ledgers.
/// The full array size is explicit in this type; choose stack or hosted storage
/// according to the application's memory budget. No allocation occurs here.
pub struct FixedStorage<
    'pool,
    'id,
    I,
    A,
    P: Copy + Eq,
    S,
    const CAPS: usize,
    const PATH: usize,
    const SLOTS: usize,
    const NODES: usize,
    const ADOPTED: usize = 0,
    const LEDGERS: usize = SLOTS,
> {
    imports: [imports::Slot<I>; LEDGERS],
    exports: [exports::Slot<clients::Client<'id>>; LEDGERS],
    questions: [questions::Slot; LEDGERS],
    adopted: [questions::AdoptedSlot; ADOPTED],
    answers: [answers::Slot<Answer<'pool, 'id, A, CAPS>>; LEDGERS],
    nodes: [clients::Slot<binding::Route<'id, P, S, PATH>, P>; NODES],
    listeners: [clients::ListenerSlot; 8],
    subscriptions: [[clients::SubscriptionSlot; 4]; NODES],
    tasks: [dispatch::TaskSlot<Task<'pool, 'id, I, CAPS, PATH>>; SLOTS],
    import_bindings: [binding::ImportSlot<'id, 'id>; SLOTS],
    pipelines: [pipelines::Slot<'id, 'id, PATH>; SLOTS],
    outgoing: [outgoing::Slot<'id, PATH>; SLOTS],
    exported: [binding::exporting::Slot<'id, 'id>; SLOTS],
    loopbacks: [super::super::loopback::Slot<'id>; SLOTS],
    embargoes: [super::super::loopback::ReceiptSlot<'id>; SLOTS],
    exchanges: [ExchangeSlot<'pool, 'id, I, CAPS>; SLOTS],
    invocation_pipelines: [InvocationPipelineSlot<'id, PATH>; SLOTS],
    operations: [OperationSlot<'id>; SLOTS],
}
impl<
        'pool,
        'id,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
        const SLOTS: usize,
        const NODES: usize,
        const ADOPTED: usize,
        const LEDGERS: usize,
    > FixedStorage<'pool, 'id, I, A, P, S, CAPS, PATH, SLOTS, NODES, ADOPTED, LEDGERS>
{
    pub const fn new() -> Self {
        Self {
            imports: [const { imports::Slot::EMPTY }; LEDGERS],
            exports: [const { exports::Slot::EMPTY }; LEDGERS],
            questions: [const { questions::Slot::EMPTY }; LEDGERS],
            adopted: [const { questions::AdoptedSlot::EMPTY }; ADOPTED],
            answers: [const { answers::Slot::EMPTY }; LEDGERS],
            nodes: [const { clients::Slot::EMPTY }; NODES],
            listeners: [const { clients::ListenerSlot::EMPTY }; 8],
            subscriptions: [const { [const { clients::SubscriptionSlot::EMPTY }; 4] }; NODES],
            tasks: [const { dispatch::TaskSlot::EMPTY }; SLOTS],
            import_bindings: [const { binding::ImportSlot::EMPTY }; SLOTS],
            pipelines: [const { pipelines::Slot::EMPTY }; SLOTS],
            outgoing: [const { outgoing::Slot::EMPTY }; SLOTS],
            exported: [const { binding::exporting::Slot::EMPTY }; SLOTS],
            loopbacks: [const { super::super::loopback::Slot::EMPTY }; SLOTS],
            embargoes: [const { super::super::loopback::ReceiptSlot::EMPTY }; SLOTS],
            exchanges: [const { ExchangeSlot::EMPTY }; SLOTS],
            invocation_pipelines: [const { InvocationPipelineSlot::EMPTY }; SLOTS],
            operations: [const { OperationSlot::EMPTY }; SLOTS],
        }
    }
    pub fn bind<'s>(
        &'s mut self,
        domain: connection::Domain<'id>,
        peer: P,
    ) -> Result<Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>, InitializeError> {
        self.bind_using(domain, peer, connection::Domain::bind_with_adoptions)
    }
    #[allow(clippy::type_complexity)]
    fn bind_using<'s>(
        &'s mut self,
        domain: connection::Domain<'id>,
        peer: P,
        initialize: impl FnOnce(
            connection::Domain<'id>,
            connection::GraphStorage<
                'id,
                's,
                I,
                Answer<'pool, 'id, A, CAPS>,
                binding::Route<'id, P, S, PATH>,
                P,
            >,
            &'s mut [questions::AdoptedSlot],
        ) -> Result<
            connection::GraphConnection<
                'id,
                's,
                I,
                Answer<'pool, 'id, A, CAPS>,
                binding::Route<'id, P, S, PATH>,
                P,
            >,
            connection::GraphError,
        >,
    ) -> Result<Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>, InitializeError> {
        let connection = initialize(
            domain,
            connection::GraphStorage {
                notifications: clients::NotificationStorage {
                    listeners: &mut self.listeners,
                    subscriptions: self.subscriptions.as_flattened_mut(),
                },
                imports: &mut self.imports,
                exports: &mut self.exports,
                questions: &mut self.questions,
                answers: &mut self.answers,
                clients: &mut self.nodes,
            },
            &mut self.adopted,
        )
        .map_err(InitializeError::Domain)?;
        Driver::new(
            connection,
            Storage {
                tasks: &mut self.tasks,
                imports: &mut self.import_bindings,
                pipelines: &mut self.pipelines,
                outgoing: &mut self.outgoing,
                exported: &mut self.exported,
                loopbacks: &mut self.loopbacks,
                embargoes: &mut self.embargoes,
                exchanges: &mut self.exchanges,
                invocation_pipelines: &mut self.invocation_pipelines,
                operations: &mut self.operations,
            },
            peer,
        )
        .map_err(|(error, _empty_connection)| InitializeError::Driver(error))
    }
}

#[cfg(feature = "alloc")]
impl<
        'pool,
        'id,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
        const SLOTS: usize,
        const ADOPTED: usize,
        const LEDGERS: usize,
    > FixedStorage<'pool, 'id, I, A, P, S, CAPS, PATH, SLOTS, 0, ADOPTED, LEDGERS>
{
    /// Keep caller-backed ledgers and dispatch while allowing the capability
    /// graph to grow fallibly. NODES is zero for this provider: no unused fixed
    /// graph array is retained. Growth preserves existing capability identities.
    /// All other capacities and the explicit graph maximum still apply.
    pub fn bind_growing_clients<'s>(
        &'s mut self,
        domain: connection::Domain<'id>,
        peer: P,
        initial: usize,
        maximum: usize,
    ) -> Result<Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>, InitializeError> {
        self.bind_using(domain, peer, |domain, storage, adopted| {
            domain.bind_growing_clients(
                connection::Storage {
                    imports: storage.imports,
                    exports: storage.exports,
                    questions: storage.questions,
                    answers: storage.answers,
                },
                adopted,
                initial,
                maximum,
                clients::NotificationGrowth::bounded(8, maximum.saturating_mul(4)),
            )
        })
    }
}
#[cfg(feature = "alloc")]
impl<
        'pool,
        'id,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
        const SLOTS: usize,
        const ADOPTED: usize,
    > FixedStorage<'pool, 'id, I, A, P, S, CAPS, PATH, SLOTS, 0, ADOPTED, 0>
{
    /// Growing ledgers and capability graph with fixed dispatch/control storage.
    /// The zero ledger/node parameters avoid retaining unused fixed arrays.
    /// Resource bounds, caller polling and output backpressure still apply.
    pub fn bind_growing<'s>(
        &'s mut self,
        domain: connection::Domain<'id>,
        peer: P,
        storage: connection::GrowingStorage,
    ) -> Result<Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>, InitializeError> {
        self.bind_using(domain, peer, |domain, _empty, adopted| {
            domain.bind_growing(storage, adopted)
        })
    }
}
impl<
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
        const SLOTS: usize,
        const NODES: usize,
        const ADOPTED: usize,
        const LEDGERS: usize,
    > Default for FixedStorage<'_, '_, I, A, P, S, CAPS, PATH, SLOTS, NODES, ADOPTED, LEDGERS>
{
    fn default() -> Self {
        Self::new()
    }
}
