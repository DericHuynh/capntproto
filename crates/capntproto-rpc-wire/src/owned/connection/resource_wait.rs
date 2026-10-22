//! Capacity readiness for retained protocol owners, after all rollback.
use super::*;
use crate::resources as r;
use capntproto_rpc_engine::clients::Capabilities;
use capntproto_rpc_engine::waiting::Requirement;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner<'id> {
    Call(answers::Key<'id>),
    Resolution(Option<imports::Key<'id>>),
    Finish(questions::Key<'id>),
    Release(imports::Key<'id>),
    Cancellation(answers::Key<'id>),
    Dispatch(dispatch::TaskKey<'id>),
    Barrier(clients::Key<'id>),
    Echo(u32),
}
#[derive(Clone, Copy)]
pub(super) struct Blocked<'id> {
    owner: Owner<'id>,
    requirement: Requirement,
}
#[derive(Clone, Copy, Default)]
pub(super) struct Available {
    values: [usize; r::COUNT],
    limits: [usize; r::COUNT],
}
impl Available {
    pub fn ready(&self, r: Requirement) -> bool {
        self.values[r.resource] >= r.minimum
    }
    fn after_release(&self, resource: usize) -> Option<Requirement> {
        let current = self.values[resource];
        (current < self.limits[resource]).then_some(Requirement {
            resource,
            minimum: current.saturating_add(1),
        })
    }
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    fn control_owner(&self, work: u32) -> Option<Owner<'id>> {
        match work {
            0 => self.call.as_ref().map(|call| Owner::Call(call.answer())),
            1 => self
                .resolution
                .as_ref()
                .map(|resolve| Owner::Resolution(resolve.target())),
            9 => self
                .ledgers
                .questions
                .pending_finish_key()
                .map(Owner::Finish),
            10 => self
                .ledgers
                .imports
                .pending_release_key()
                .map(Owner::Release),
            11 => self
                .ledgers
                .answers
                .next_queued_cancellation()
                .map(Owner::Cancellation),
            12 => self.runtime.ready_key().map(Owner::Dispatch),
            13 => self
                .barrier
                .as_ref()
                .map(|barrier| Owner::Barrier(barrier.key())),
            14 => self.echo.as_ref().map(|echo| Owner::Echo(echo.context())),
            _ => None,
        }
    }
    pub(super) fn filter_control_waits(
        &self,
        mut ready: u32,
        messages: &messages::Messages<'pool, '_, '_>,
        queue: &messages::Queue<'pool, '_, impl Sized>,
    ) -> u32 {
        let mut waiting = ready & self.control_waiting;
        while waiting != 0 {
            let work = waiting.trailing_zeros();
            waiting &= waiting - 1;
            if work == 0 && self.runtime.dispatch_is_pending() {
                continue;
            }
            if work == 11 {
                if let Some(key) = self.ledgers.answers.next_queued_cancellation() {
                    let retained = self.call.as_ref().is_some_and(|call| call.answer() == key);
                    let queued = self
                        .ledgers
                        .answers
                        .get(key)
                        .ok()
                        .and_then(|state| state.owner.queued)
                        .and_then(|task| self.runtime.queued_owner(task))
                        .and_then(Task::as_call)
                        .is_some_and(|call| call.answer() == key);
                    if !retained && !queued {
                        continue;
                    }
                }
            }
            let blocked = self.control_waits[work as usize].as_ref().unwrap();
            if self.control_owner(work) == Some(blocked.owner)
                && !crate::resources::message_ready(blocked.requirement, messages, queue)
                && !self.available_resources().ready(blocked.requirement)
            {
                ready &= !(1 << work);
            }
        }
        ready
    }
    pub(super) fn park_control(
        &mut self,
        work: u32,
        error: Error,
        messages: &messages::Messages<'pool, '_, '_>,
        queue: &messages::Queue<'pool, '_, impl Sized>,
    ) -> bool {
        let Some(owner) = self.control_owner(work) else {
            return false;
        };
        let requirement = match error {
            Error::Input(super::super::Error::Storage(e))
            | Error::Input(super::super::Error::Output(crate::admission::Error::Storage(e))) => {
                crate::resources::message_wait(e, messages, queue)
            }
            _ => self.resource_wait(error),
        };
        let Some(requirement) = requirement else {
            return false;
        };
        self.control_waits[work as usize] = Some(Blocked { owner, requirement });
        self.control_waiting |= 1 << work;
        true
    }
    pub(super) fn available_resources(&self) -> Available {
        let graph = self.runtime.resources();
        let mut a = Available::default();
        for (resource, available, limit) in [
            (r::NODES, graph.nodes, graph.node_limit),
            (r::LISTENERS, graph.listeners, graph.listener_limit),
            (
                r::SUBSCRIPTIONS,
                graph.subscriptions,
                graph.subscription_limit,
            ),
            (
                r::OUTGOING,
                self.outgoing.available(),
                self.outgoing.maximum(),
            ),
            (
                r::PIPELINES,
                self.pipelines.available(),
                self.pipelines.maximum(),
            ),
            (
                r::BARRIERS,
                if self.barrier.is_some() {
                    0
                } else {
                    self.loopbacks.available()
                },
                self.loopbacks.maximum(),
            ),
            (
                r::TASKS,
                self.runtime.available_tasks(),
                self.runtime.maximum_tasks(),
            ),
            (
                r::IMPORT_BINDINGS,
                self.ledgers.imports.binding_revision(),
                if self.imports.maximum() == 0 {
                    0
                } else {
                    usize::MAX
                },
            ),
        ] {
            a.values[resource] = available;
            a.limits[resource] = limit;
        }
        a
    }
    pub(super) fn resource_wait(&self, error: Error) -> Option<Requirement> {
        fn graph(e: clients::Error) -> Option<usize> {
            match e {
                clients::Error::Full => Some(r::NODES),
                clients::Error::ListenersFull => Some(r::LISTENERS),
                clients::Error::SubscriptionsFull => Some(r::SUBSCRIPTIONS),
                _ => None,
            }
        }
        fn pipeline(e: pipelines::Error) -> Option<usize> {
            match e {
                pipelines::Error::Graph(e) => graph(e),
                pipelines::Error::ObserversFull => Some(r::PIPELINES),
                _ => None,
            }
        }
        fn binding(e: binding::BindError) -> Option<usize> {
            match e {
                binding::BindError::Graph(e) => graph(e),
                binding::BindError::Pipeline(e) => pipeline(e),
                binding::BindError::Imports(binding::Error::Graph(e)) => graph(e),
                binding::BindError::Imports(binding::Error::Full) => Some(r::IMPORT_BINDINGS),
                _ => None,
            }
        }
        fn dispatch(e: dispatch::DispatchError) -> Option<usize> {
            match e {
                dispatch::DispatchError::Full => Some(r::TASKS),
                dispatch::DispatchError::Graph(e) => graph(e),
                _ => None,
            }
        }
        let resource = match error {
            Error::Graph(e) => graph(e),
            Error::Pipeline(e) => pipeline(e),
            Error::Dispatch(e) => dispatch(e),
            Error::BarrierFull => Some(r::BARRIERS),
            Error::Outgoing(outgoing::Error::Graph(e)) => graph(e),
            Error::Outgoing(outgoing::Error::Binding(e)) => binding(e),
            Error::Outgoing(outgoing::Error::ObserversFull) => Some(r::OUTGOING),
            Error::Enqueue(EnqueueError::Graph(e)) => graph(e),
            Error::Enqueue(EnqueueError::Pipeline(e)) => pipeline(e),
            Error::Enqueue(EnqueueError::Dispatch(e)) => dispatch(e),
            Error::Resolve(ApplyError::Graph(e)) => graph(e),
            Error::Resolve(ApplyError::Binding(e)) => binding(e),
            _ => None,
        }?;
        self.available_resources().after_release(resource)
    }
}
