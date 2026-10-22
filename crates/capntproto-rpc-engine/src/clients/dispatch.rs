//! Bounded, caller-polled dispatch over the capability graph. Every call enters
//! one sequence before dispatch; opening a barrier cannot let a new call bypass
//! already queued calls. Unrelated ready targets can progress past blocked ones.
use super::*;

pub struct TaskSlot<C> {
    entry: Option<Queued<C>>,
}
impl<C> TaskSlot<C> {
    pub const EMPTY: Self = Self { entry: None };
}
struct Queued<C> {
    target: Raw,
    sequence: u64,
    owner: C,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskKey<'id> {
    sequence: u64,
    brand: Brand<'id>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchError {
    Graph(Error),
    Full,
    SequencesExhausted,
    Missing,
}
impl core::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RPC dispatch: {self:?}")
    }
}
impl core::error::Error for DispatchError {}

pub struct Storage<'s, T, P, C> {
    pub clients: &'s mut [Slot<T, P>],
    pub tasks: &'s mut [TaskSlot<C>],
}
/// Sealed graph ownership used by the owning and borrowing dispatch scopes.
/// The borrowed form lets connection receipts live inside an existing graph
/// domain without putting self-referential handles into its outer storage.
pub trait GraphStorage<'id, 'nodes, T, P>: storage_sealed::Sealed {
    fn graph(&self) -> &Graph<'id, 'nodes, T, P>;
    fn graph_mut(&mut self) -> &mut Graph<'id, 'nodes, T, P>;
}
mod storage_sealed {
    pub trait Sealed {}
}
impl<T, P> storage_sealed::Sealed for Graph<'_, '_, T, P> {}
impl<'id, 'nodes, T, P> GraphStorage<'id, 'nodes, T, P> for Graph<'id, 'nodes, T, P> {
    fn graph(&self) -> &Graph<'id, 'nodes, T, P> {
        self
    }
    fn graph_mut(&mut self) -> &mut Graph<'id, 'nodes, T, P> {
        self
    }
}
impl<T, P> storage_sealed::Sealed for &mut Graph<'_, '_, T, P> {}
impl<'id, 'nodes, T, P> GraphStorage<'id, 'nodes, T, P> for &mut Graph<'id, 'nodes, T, P> {
    fn graph(&self) -> &Graph<'id, 'nodes, T, P> {
        self
    }
    fn graph_mut(&mut self) -> &mut Graph<'id, 'nodes, T, P> {
        self
    }
}

pub struct Runtime<
    'id,
    'nodes,
    'tasks,
    T: 'nodes,
    P: Copy + Eq + 'nodes,
    C,
    G: GraphStorage<'id, 'nodes, T, P> = Graph<'id, 'nodes, T, P>,
> {
    graph: G,
    tasks: crate::storage::Storage<'tasks, TaskSlot<C>>,
    queued: usize,
    brand: Brand<'id>,
    storage: PhantomData<&'nodes (T, P)>,
}
fn new_scope<T, P: Copy + Eq, C>(
    storage: Storage<'_, T, P, C>,
) -> Result<Runtime<'_, '_, '_, T, P, C>, Error> {
    if storage.clients.iter().any(|s| s.entry.is_some())
        || storage.tasks.iter().any(|s| s.entry.is_some())
    {
        return Err(Error::NonemptyStorage);
    }
    Ok(Runtime {
        graph: Graph {
            slots: crate::storage::Storage::borrowed(storage.clients),
            dispatch_sequence: 0,
            retired_head: None,
            retired_tail: None,
            brand: PhantomData,
        },
        tasks: crate::storage::Storage::borrowed(storage.tasks),
        queued: 0,
        brand: PhantomData,
        storage: PhantomData,
    })
}
pub fn with_runtime<T, P: Copy + Eq, C, R>(
    storage: Storage<'_, T, P, C>,
    f: impl for<'id> FnOnce(&mut Runtime<'id, '_, '_, T, P, C>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(storage)?))
}
pub async fn with_runtime_async<T, P: Copy + Eq, C, R>(
    storage: Storage<'_, T, P, C>,
    f: impl for<'id> core::ops::AsyncFnOnce(&mut Runtime<'id, '_, '_, T, P, C>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(storage)?).await)
}

/// Borrow an established graph after the connection's export/answer domains
/// exist. Task owners can then contain those domains' affine receipts. The graph
/// stays borrowed until this dispatcher ends; sequence IDs persist across such
/// scopes, so an earlier task key cannot cancel a later task in recycled slots.
/// Drain queued owners before a normal shutdown. Dropping a nonempty dispatcher
/// is terminal teardown: its graph references are released, but application
/// owners may require ending their enclosing connection/message scopes too.
#[allow(clippy::type_complexity)]
pub fn borrow_runtime<'g, 'id, 'nodes, 'tasks, T, P: Copy + Eq, C>(
    graph: &'g mut Graph<'id, 'nodes, T, P>,
    tasks: &'tasks mut [TaskSlot<C>],
) -> Result<Runtime<'id, 'nodes, 'tasks, T, P, C, &'g mut Graph<'id, 'nodes, T, P>>, Error> {
    if tasks.iter().any(|slot| slot.entry.is_some()) {
        return Err(Error::NonemptyStorage);
    }
    Ok(Runtime {
        graph,
        tasks: crate::storage::Storage::borrowed(tasks),
        queued: 0,
        brand: PhantomData,
        storage: PhantomData,
    })
}

/// Attach caller-owned task storage to an established graph without borrowing
/// the graph from its enclosing connection. Its identity, references and task
/// sequence remain unchanged. Rejection returns the intact graph.
#[allow(clippy::type_complexity)]
pub fn own_runtime<'id, 'nodes, 'tasks, T, P: Copy + Eq, C>(
    graph: Graph<'id, 'nodes, T, P>,
    tasks: &'tasks mut [TaskSlot<C>],
) -> Result<Runtime<'id, 'nodes, 'tasks, T, P, C>, Rejected<Graph<'id, 'nodes, T, P>>> {
    own_runtime_with_storage(graph, crate::storage::Storage::borrowed(tasks))
}

/// Attach fixed or fallibly growing task storage. No live graph is consumed on
/// rejection. Task indices may relocate; accepted sequence identities cannot.
#[allow(clippy::type_complexity)]
pub fn own_runtime_with_storage<'id, 'nodes, 'tasks, T, P: Copy + Eq, C>(
    graph: Graph<'id, 'nodes, T, P>,
    tasks: crate::storage::Storage<'tasks, TaskSlot<C>>,
) -> Result<Runtime<'id, 'nodes, 'tasks, T, P, C>, Rejected<Graph<'id, 'nodes, T, P>>> {
    if tasks.iter().any(|slot| slot.entry.is_some()) {
        return Err(Rejected {
            error: Error::NonemptyStorage,
            value: graph,
        });
    }
    Ok(Runtime {
        graph,
        tasks,
        queued: 0,
        brand: PhantomData,
        storage: PhantomData,
    })
}

#[must_use]
pub struct Enqueue<'a, 'id, 's, T, P, C> {
    call: Call<'a, 'id, 's, T, P>,
    slot: &'a mut TaskSlot<C>,
    owner: &'a mut Option<C>,
    queued: &'a mut usize,
}
impl<'id, T, P: Copy + Eq, C> Enqueue<'_, 'id, '_, T, P, C> {
    pub fn accept(self) -> TaskKey<'id> {
        *self.queued += 1;
        let sequence = self.call.graph.dispatch_sequence;
        self.call.graph.dispatch_sequence += 1;
        let target = self.call.accept().key.raw;
        self.slot.entry = Some(Queued {
            target,
            sequence,
            owner: self.owner.take().unwrap(),
        });
        TaskKey {
            sequence,
            brand: PhantomData,
        }
    }
}

/// Exclusive output admission. Dropping this view keeps the call in its place,
/// including its payload and capability reference, for a later poll.
#[must_use]
pub struct Ready<'a, 'id, 's, T, P, C> {
    call: Call<'a, 'id, 's, T, P>,
    slot: &'a mut TaskSlot<C>,
    queued: &'a mut usize,
}

/// A routable call admitted without an intermediate queued task. This is only
/// offered when the dispatch queue is empty. Keep the ticket until output has
/// accepted the call; dropping it changes no ordering or promise-use state.
#[must_use]
pub struct Immediate<'a, 'id, 's, T, P> {
    call: Call<'a, 'id, 's, T, P>,
}
impl<'id, T, P: Copy + Eq> Immediate<'_, 'id, '_, T, P> {
    /// Commit a directly dispatched operation with the same target ownership
    /// as `Ready::accept`, without inserting and removing an intermediate task.
    #[inline(always)]
    pub fn accept_owned<C>(self, owner: C) -> Running<'id, C> {
        Running {
            target: self.accept(),
            owner,
        }
    }
    pub fn destination(&self) -> Destination<'id, '_, T, P> {
        self.call.destination()
    }
    /// Commit dispatch after successful publication, retaining the destination.
    /// Release the returned client through the same runtime after use.
    pub fn accept(self) -> Client<'id> {
        self.call.graph.dispatch_sequence += 1;
        self.call.accept()
    }
    /// Commit a synchronously published call whose transport already owns its
    /// destination. This has the same ordering and promise-use effects as
    /// `accept` followed by releasing its client, without a temporary reference.
    pub fn accept_published(self) {
        self.call.graph.dispatch_sequence += 1;
        let mut current = self.call.start;
        // Preflight reached an endpoint. Only ancestors can be promises,
        // and the exclusive ticket keeps the original route unchanged.
        while current != self.call.end {
            let Kind::Promise {
                called,
                target: Some(next),
                ..
            } = &mut self.call.graph.slots[current.index]
                .entry
                .as_mut()
                .unwrap()
                .kind
            else {
                unreachable!("validated endpoint route")
            };
            *called = true;
            current = *next;
        }
    }
}
#[must_use = "complete through the runtime to release its target"]
pub struct Running<'id, C> {
    target: Client<'id>,
    owner: C,
}
impl<'id, C> Running<'id, C> {
    /// The destination frozen when this operation was accepted. Borrowing it
    /// grants no additional reference and cannot retarget the running call.
    pub fn target(&self) -> &Client<'id> {
        &self.target
    }
    pub fn owner(&self) -> &C {
        &self.owner
    }
    pub fn owner_mut(&mut self) -> &mut C {
        &mut self.owner
    }
    /// Change the application's owner without changing the frozen destination
    /// or its retained graph reference. Complete the returned running token.
    pub fn map_owner<U>(self, map: impl FnOnce(C) -> U) -> Running<'id, U> {
        Running {
            target: self.target,
            owner: map(self.owner),
        }
    }
}
impl<'id, T, P: Copy + Eq, C> Ready<'_, 'id, '_, T, P, C> {
    pub fn key(&self) -> TaskKey<'id> {
        TaskKey {
            sequence: self.slot.entry.as_ref().unwrap().sequence,
            brand: PhantomData,
        }
    }
    pub fn destination(&self) -> Destination<'id, '_, T, P> {
        self.call.destination()
    }
    pub fn owner(&self) -> &C {
        &self.slot.entry.as_ref().unwrap().owner
    }
    pub fn accept(self) -> Running<'id, C> {
        *self.queued -= 1;
        let queued = self.slot.entry.take().unwrap();
        // Freeze the destination while accepting into caller-owned execution or
        // transport output. Future promise resolution cannot redirect this task.
        let target = self.call.accept_releasing(Some(queued.target));
        Running {
            target,
            owner: queued.owner,
        }
    }
}

impl<'id, 's, T: 's, P: Copy + Eq + 's, C, G: GraphStorage<'id, 's, T, P>>
    Runtime<'id, 's, '_, T, P, C, G>
{
    pub fn endpoint(
        &mut self,
        owner: T,
        location: Location<P>,
    ) -> Result<Client<'id>, Rejected<T>> {
        self.graph.graph_mut().endpoint(owner, location)
    }
    pub fn promise(&mut self, initial: &mut Option<Client<'id>>) -> Result<Client<'id>, Error> {
        self.graph.graph_mut().promise(initial)
    }
    pub fn retain(&mut self, client: &Client<'id>) -> Result<Client<'id>, Error> {
        self.graph.graph_mut().retain(client)
    }
    pub fn snapshot(&mut self, client: &Client<'id>) -> Result<Client<'id>, Error> {
        self.graph.graph_mut().snapshot(client)
    }
    pub fn release(&mut self, client: Client<'id>) {
        self.graph.graph_mut().release(client);
    }
    pub fn prepare_resolution<'a>(
        &'a mut self,
        key: Key<'id>,
        replacement: &'a mut Option<Client<'id>>,
        source: Source,
    ) -> Result<Resolution<'a, 'id, 's, T, P>, Error> {
        self.graph
            .graph_mut()
            .prepare_resolution(key, replacement, source)
    }
    pub fn barrier_route(&self, barrier: &Barrier<'id>) -> &T {
        self.graph.graph().barrier_route(barrier)
    }
    /// Queue the senderLoopback on this retained original endpoint in the same
    /// dispatch sequence as older calls. Pair the affine barrier with its wire
    /// embargo ID; the receiving adapter opens it only on a matching echo.
    pub fn retain_barrier_route(&mut self, barrier: &Barrier<'id>) -> Result<Client<'id>, Error> {
        let Kind::Promise {
            prior: Some(prior), ..
        } = self.graph.graph().live(barrier.key().raw)?.kind
        else {
            unreachable!()
        };
        let graph = self.graph.graph_mut();
        graph.retain_key(graph.key(prior))
    }
    pub fn open(&mut self, barrier: Barrier<'id>) {
        self.graph.graph_mut().open(barrier);
    }
    pub fn prepare_call<'a>(
        &'a mut self,
        target: &Client<'id>,
        owner: &'a mut Option<C>,
    ) -> Result<Enqueue<'a, 'id, 's, T, P, C>, DispatchError> {
        if owner.is_none() {
            return Err(DispatchError::Missing);
        }
        if self.graph.graph().dispatch_sequence == u64::MAX {
            return Err(DispatchError::SequencesExhausted);
        }
        let slot = self
            .tasks
            .vacant_mut(|_, s| s.entry.is_none(), || TaskSlot::EMPTY)
            .ok_or(DispatchError::Full)?;
        let call = self
            .graph
            .graph_mut()
            .prepare_call(target)
            .map_err(DispatchError::Graph)?;
        Ok(Enqueue {
            call,
            slot,
            owner,
            queued: &mut self.queued,
        })
    }
    /// Offer direct dispatch only for settled routes with no older queued work.
    /// Pending promises and queued loopbacks always use ordinary ordered tasks.
    /// Sequence/refcount exhaustion is checked before the caller publishes.
    pub fn prepare_immediate(
        &mut self,
        target: &Client<'id>,
    ) -> Result<Option<Immediate<'_, 'id, 's, T, P>>, DispatchError> {
        if self.queued != 0 {
            return Ok(None);
        }
        // The ticket borrows this graph exclusively, so its validated terminal
        // route cannot change before commit. Resolve it only once.
        let sequence_exhausted = self.graph.graph().dispatch_sequence == u64::MAX;
        let Some(call) = self
            .graph
            .graph_mut()
            .prepare_settled_call(target)
            .map_err(DispatchError::Graph)?
        else {
            return Ok(None);
        };
        if sequence_exhausted {
            return Err(DispatchError::SequencesExhausted);
        }
        Ok(Some(Immediate { call }))
    }
    /// Offer direct dispatch along a currently callable route, including an
    /// unresolved remote promise's original route. Acceptance marks its promise
    /// ancestors used, so subsequent reflected resolution still needs its
    /// ordering barrier. Waiting routes and older queued tasks defer unchanged.
    /// The exclusive ticket must survive until the exact output is accepted.
    pub fn prepare_immediate_routed(
        &mut self,
        target: &Client<'id>,
    ) -> Result<Option<Immediate<'_, 'id, 's, T, P>>, DispatchError> {
        if self.queued != 0 {
            return Ok(None);
        }
        let sequence_exhausted = self.graph.graph().dispatch_sequence == u64::MAX;
        let call = self
            .graph
            .graph_mut()
            .prepare_call(target)
            .map_err(DispatchError::Graph)?;
        if call.waiting {
            return Ok(None);
        }
        if sequence_exhausted {
            return Err(DispatchError::SequencesExhausted);
        }
        Ok(Some(Immediate { call }))
    }
    /// Select the oldest ready call, without consuming it. Failed output
    /// admission retries the same call; waiting promises do not block other vats.
    pub fn next_ready(&mut self) -> Result<Option<Ready<'_, 'id, 's, T, P, C>>, DispatchError> {
        if self.queued == 0 {
            return Ok(None);
        }
        let mut selected: Option<(usize, u64, Raw)> = None;
        for (index, slot) in self.tasks.iter().enumerate() {
            if let Some(queued) = &slot.entry {
                let (end, waiting, _) = self
                    .graph
                    .graph()
                    .walk(queued.target)
                    .map_err(DispatchError::Graph)?;
                if !waiting && selected.is_none_or(|(_, sequence, _)| queued.sequence < sequence) {
                    selected = Some((index, queued.sequence, end));
                }
            }
        }
        let Some((index, _, end)) = selected else {
            return Ok(None);
        };
        let slot = &mut self.tasks[index];
        let graph = self.graph.graph_mut();
        // The scan and this offer share one exclusive runtime borrow. No route
        // can change between them, so reuse its validated terminal endpoint.
        if graph.live(end).map_err(DispatchError::Graph)?.refs == u32::MAX {
            return Err(DispatchError::Graph(Error::ReferencesExhausted));
        }
        let call = Call {
            graph,
            start: slot.entry.as_ref().unwrap().target,
            end,
            waiting: false,
        };
        Ok(Some(Ready {
            call,
            slot,
            queued: &mut self.queued,
        }))
    }
    #[inline(always)]
    pub fn complete<U>(&mut self, task: Running<'id, U>) -> U {
        self.graph.graph_mut().release(task.target);
        task.owner
    }
    pub fn cancel(&mut self, key: TaskKey<'id>) -> Result<C, DispatchError> {
        let slot = self
            .tasks
            .iter_mut()
            .find(|s| s.entry.as_ref().is_some_and(|e| e.sequence == key.sequence))
            .ok_or(DispatchError::Missing)?;
        let entry = slot.entry.take().unwrap();
        self.queued -= 1;
        self.graph.graph_mut().release_raw(entry.target);
        Ok(entry.owner)
    }
    pub fn take_retired(&mut self) -> Option<T> {
        self.graph.graph_mut().take_retired()
    }
    pub fn has_retired(&self) -> bool {
        self.graph.graph().has_retired()
    }
    /// Detach one queued owner even when its promise cannot resolve. Dispose of
    /// the owner's connection receipts after this borrow ends. No IO, wakeup or
    /// application destructor runs inside this transition.
    pub fn drain_one(&mut self) -> Option<C> {
        if self.queued == 0 {
            return None;
        }
        let slot = self.tasks.iter_mut().find(|slot| slot.entry.is_some())?;
        let entry = slot.entry.take().unwrap();
        self.queued -= 1;
        self.graph.graph_mut().release_raw(entry.target);
        Some(entry.owner)
    }
    pub fn queued(&self) -> usize {
        self.queued
    }
    /// Inspect one caller-backed task slot without making its target runnable
    /// or changing ordering. The key is generation-safe across slot reuse.
    pub fn queued_at(&self, index: usize) -> Option<(TaskKey<'id>, &C)> {
        let entry = self.tasks.get(index)?.entry.as_ref()?;
        Some((
            TaskKey {
                sequence: entry.sequence,
                brand: PhantomData,
            },
            &entry.owner,
        ))
    }
    /// Mutate the owner of an exact queued task without changing its route or
    /// dispatch order. Published or reused task identities cannot match.
    pub fn queued_owner_mut(&mut self, key: TaskKey<'id>) -> Option<&mut C> {
        self.tasks
            .iter_mut()
            .filter_map(|slot| slot.entry.as_mut())
            .find(|entry| entry.sequence == key.sequence)
            .map(|entry| &mut entry.owner)
    }
    /// Reserve enough slots for the next admission, including protocol headroom.
    /// Failure does not consume owners, references or task sequence identities.
    pub fn reserve_task_capacity(&mut self, required: usize) -> bool {
        required <= self.tasks.len()
            || self
                .tasks
                .grow(required - self.tasks.len(), || TaskSlot::EMPTY)
    }
    pub fn task_capacity(&self) -> usize {
        self.tasks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.queued() == 0 && self.graph.graph().is_empty()
    }
}
impl<'id, 'nodes, T: 'nodes, P: Copy + Eq + 'nodes, C, G: GraphStorage<'id, 'nodes, T, P>> Drop
    for Runtime<'id, 'nodes, '_, T, P, C, G>
{
    fn drop(&mut self) {
        for slot in &mut *self.tasks {
            if let Some(entry) = slot.entry.take() {
                self.graph.graph_mut().release_raw(entry.target);
                drop(entry.owner);
            }
        }
    }
}
mod terminated;
#[cfg(test)]
mod tests;
pub use terminated::{TerminateError, Terminated};

#[cfg(kani)]
mod proofs {
    use super::*;
    #[kani::proof]
    #[kani::unwind(4)]
    fn borrowed_scopes_never_reuse_accepted_task_identity() {
        let sequence: u64 = kani::any();
        kani::assume(sequence <= u64::MAX - 2);
        let mut nodes = [Slot::EMPTY];
        with_graph(&mut nodes, |graph: &mut Graph<'_, '_, u8, u8>| {
            graph.dispatch_sequence = sequence;
            let client = graph.endpoint(41, Location::Local).unwrap();
            let mut tasks = [TaskSlot::EMPTY];
            let old = {
                let mut runtime = borrow_runtime(graph, &mut tasks).unwrap();
                let key = runtime
                    .prepare_call(&client, &mut Some(43u8))
                    .unwrap()
                    .accept();
                assert_eq!(runtime.cancel(key), Ok(43));
                key
            };
            let mut runtime = borrow_runtime(graph, &mut tasks).unwrap();
            let key = runtime
                .prepare_call(&client, &mut Some(47))
                .unwrap()
                .accept();
            kani::assert(
                key != old,
                "borrowed dispatcher never reuses prior task identity",
            );
            assert_eq!(runtime.cancel(old), Err(DispatchError::Missing));
            assert_eq!(runtime.cancel(key), Ok(47));
            drop(runtime);
            graph.release(client);
            assert_eq!(graph.take_retired(), Some(41));
            assert!(graph.is_empty());
        })
        .unwrap();
    }
    #[kani::proof]
    #[kani::unwind(4)]
    fn oldest_ready_call_survives_rejection_and_slot_reuse() {
        let first = 17u8;
        let second = 23u8;
        let reject: bool = kani::any();
        let mut clients = [Slot::EMPTY];
        let mut tasks = [const { TaskSlot::EMPTY }; 2];
        with_runtime(
            Storage {
                clients: &mut clients,
                tasks: &mut tasks,
            },
            |runtime: &mut Runtime<'_, '_, '_, u8, u8, u8>| {
                let client = runtime.endpoint(0, Location::Local).unwrap();
                // Reachable after calls 0 and 1 were queued, call 0 completed,
                // and its physical slot was reused by call 2. The endpoint has
                // one application reference and one from each queued call.
                runtime.graph.slots[0].entry.as_mut().unwrap().refs = 3;
                runtime.tasks[0].entry = Some(Queued {
                    target: client.key.raw,
                    sequence: 2,
                    owner: second,
                });
                runtime.tasks[1].entry = Some(Queued {
                    target: client.key.raw,
                    sequence: 1,
                    owner: first,
                });
                runtime.graph.dispatch_sequence = 3;
                runtime.queued = 2;
                let old = TaskKey {
                    sequence: 0,
                    brand: PhantomData,
                };
                let new = TaskKey {
                    sequence: 2,
                    brand: PhantomData,
                };
                kani::assert(
                    runtime.cancel(old) == Err(DispatchError::Missing),
                    "stale task cannot cancel a reused slot",
                );
                if reject {
                    drop(runtime.next_ready().unwrap().unwrap());
                }
                let ready = runtime.next_ready().unwrap().unwrap();
                kani::assert(
                    *ready.owner() == first,
                    "dispatch preserves the oldest ready call",
                );
                let running = ready.accept();
                assert_eq!(runtime.complete(running), first);
                assert_eq!(runtime.cancel(new), Ok(second));
                runtime.release(client);
                assert_eq!(runtime.take_retired(), Some(0));
                assert!(runtime.is_empty());
            },
        )
        .unwrap();
    }
}

impl<'id, 's, T: 's, P: Copy + Eq + 's, C, G: GraphStorage<'id, 's, T, P>>
    super::store::sealed::Sealed for Runtime<'id, 's, '_, T, P, C, G>
{
}
impl<'id, 's, T: 's, P: Copy + Eq + 's, C, G: GraphStorage<'id, 's, T, P>> Capabilities<'id>
    for Runtime<'id, 's, '_, T, P, C, G>
{
    type Owner = T;
    type Peer = P;
    fn is_settled(&self, client: &Client<'id>) -> Result<bool, Error> {
        self.graph.graph().is_settled(client)
    }
    fn inspect(&self, client: &Client<'id>) -> Result<Destination<'id, '_, T, P>, Error> {
        self.graph.graph().inspect(client)
    }
    fn endpoint_with(
        &mut self,
        location: Location<P>,
        owner: impl FnOnce() -> T,
    ) -> Result<Client<'id>, Error> {
        self.graph.graph_mut().endpoint_with(location, owner)
    }
    fn snapshot(&mut self, client: &Client<'id>) -> Result<Client<'id>, Error> {
        self.snapshot(client)
    }
    fn snapshot_for_export(&mut self, client: &Client<'id>) -> Result<(Client<'id>, bool), Error> {
        self.graph.graph_mut().snapshot_for_export(client)
    }
    fn release(&mut self, client: Client<'id>) {
        self.release(client)
    }
    fn promise(&mut self, initial: &mut Option<Client<'id>>) -> Result<Client<'id>, Error> {
        self.promise(initial)
    }
    fn resolve(
        &mut self,
        key: Key<'id>,
        replacement: &mut Option<Client<'id>>,
        source: Source,
    ) -> Result<Option<Barrier<'id>>, Error> {
        Ok(self.prepare_resolution(key, replacement, source)?.accept())
    }
    fn retain_key(&mut self, key: Key<'id>) -> Result<Client<'id>, Error> {
        self.graph.graph_mut().retain_key(key)
    }
    fn endpoint(&mut self, owner: T, location: Location<P>) -> Result<Client<'id>, Rejected<T>> {
        self.graph.graph_mut().endpoint(owner, location)
    }
    fn imported_promise(&mut self, owner: T, peer: P) -> Result<Client<'id>, Rejected<T>> {
        self.graph.graph_mut().imported_promise(owner, peer)
    }
}
