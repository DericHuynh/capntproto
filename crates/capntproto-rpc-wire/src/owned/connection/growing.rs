//! Optional allocation-backed dispatch and bindings, with explicit per-table bounds.
use super::*;

/// Independent initial capacities and hard limits for the driver's resource
/// tables. All start empty; growth is fallible and happens before admission.
/// Task maximum must be at least two; loopback and embargo maxima at least one.
/// Message buffers, output FIFO, completion slots and application tasks are
/// separate caller-owned resources. Growing this provider never grows those.
#[derive(Clone, Copy, Debug)]
pub struct GrowingStorage {
    /// Per-message descriptor receipt budget. Inline keeps the const bound.
    pub message_capabilities: Capacity,
    /// Per-message non-noop pipeline transform budget.
    pub message_path: Capacity,
    pub tasks: connection::Growth,
    pub imports: connection::Growth,
    pub pipelines: connection::Growth,
    pub outgoing: connection::Growth,
    pub exported: connection::Growth,
    pub loopbacks: connection::Growth,
    pub embargoes: connection::Growth,
    pub exchanges: connection::Growth,
    pub tails: connection::Growth,
    pub invocation_pipelines: connection::Growth,
}
impl GrowingStorage {
    /// Opt into bounded per-message growth independently of resource-table
    /// growth. The capability budget includes the target receipt of an incoming
    /// Call. Path length counts only `getPointerField` operations, excluding
    /// no-ops. Const `CAPS`/`PATH` remain inline capacities, including zero.
    pub const fn with_messages(mut self, capabilities: usize, path: usize) -> Self {
        self.message_capabilities = Capacity::Growing {
            maximum: capabilities,
        };
        self.message_path = Capacity::Growing { maximum: path };
        self
    }
    /// Use a common bound for each table. For unequal budgets, set fields
    /// independently before constructing the driver.
    pub const fn uniform(initial: usize, maximum: usize) -> Self {
        let bound = connection::Growth { initial, maximum };
        Self {
            message_capabilities: Capacity::Inline,
            message_path: Capacity::Inline,
            tasks: bound,
            imports: bound,
            pipelines: bound,
            outgoing: bound,
            exported: bound,
            loopbacks: bound,
            embargoes: bound,
            exchanges: bound,
            tails: bound,
            invocation_pipelines: bound,
        }
    }
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Attach growing dispatch and binding storage to an established domain.
    /// Failure returns the original connection with all its owners unchanged.
    /// Every allocation is bounded; allocation failure is ordinary backpressure.
    #[allow(clippy::type_complexity, clippy::result_large_err)]
    pub fn new_growing(
        connection: connection::GraphConnection<
            'id,
            's,
            I,
            Answer<'pool, 'id, A, CAPS>,
            binding::Route<'id, P, S, PATH>,
            P,
        >,
        storage: GrowingStorage,
        peer: P,
    ) -> Result<
        Self,
        (
            Error,
            connection::GraphConnection<
                'id,
                's,
                I,
                Answer<'pool, 'id, A, CAPS>,
                binding::Route<'id, P, S, PATH>,
                P,
            >,
        ),
    > {
        if storage.tasks.maximum < 2
            || storage.loopbacks.maximum == 0
            || storage.embargoes.maximum == 0
        {
            return Err((Error::InsufficientControlStorage, connection));
        }
        let prepared = (|| {
            Some(PreparedStorage {
                message_capabilities: storage.message_capabilities,
                message_path: storage.message_path,
                tasks: SlotStorage::growing(storage.tasks.initial, storage.tasks.maximum, || {
                    dispatch::TaskSlot::EMPTY
                })?,
                imports: SlotStorage::growing(
                    storage.imports.initial,
                    storage.imports.maximum,
                    || binding::ImportSlot::EMPTY,
                )?,
                pipelines: SlotStorage::growing(
                    storage.pipelines.initial,
                    storage.pipelines.maximum,
                    || pipelines::Slot::EMPTY,
                )?,
                outgoing: SlotStorage::growing(
                    storage.outgoing.initial,
                    storage.outgoing.maximum,
                    || outgoing::Slot::EMPTY,
                )?,
                exported: SlotStorage::growing(
                    storage.exported.initial,
                    storage.exported.maximum,
                    || binding::exporting::Slot::EMPTY,
                )?,
                loopbacks: SlotStorage::growing(
                    storage.loopbacks.initial,
                    storage.loopbacks.maximum,
                    || super::super::loopback::Slot::EMPTY,
                )?,
                embargoes: SlotStorage::growing(
                    storage.embargoes.initial,
                    storage.embargoes.maximum,
                    || super::super::loopback::ReceiptSlot::EMPTY,
                )?,
                exchanges: SlotStorage::growing(
                    storage.exchanges.initial,
                    storage.exchanges.maximum,
                    || ExchangeSlot::EMPTY,
                )?,
                tails: SlotStorage::growing(storage.tails.initial, storage.tails.maximum, || {
                    TailSlot::EMPTY
                })?,
                invocation_pipelines: SlotStorage::growing(
                    storage.invocation_pipelines.initial,
                    storage.invocation_pipelines.maximum,
                    || InvocationPipelineSlot::EMPTY,
                )?,
            })
        })();
        match prepared {
            Some(storage) => Self::from_storage(connection, storage, peer),
            None => Err((Error::Input(super::super::Error::Capacity), connection)),
        }
    }
}

#[cfg(test)]
mod tests;
